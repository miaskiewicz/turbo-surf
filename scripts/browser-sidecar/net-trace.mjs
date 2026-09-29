// Reusable real-Chrome network tracer. Loads any URL in a real headed Chrome and records the
// full network cascade — every request (method, URL, resource type), every response that sets a
// cookie, and every POST (token/beacon exchanges) — so you can diff what a real browser fires on
// a page against what turbo-surf fires in-isolate. Not google-specific; point it at any page.
//
// Usage:
//   node net-trace.mjs <url> [--wait <ms>] [--fill "<sel>=<text>"]... [--cookie "<n>=<v>[;<domain>]"]...
//                            [--lang <tag>] [--tz <zone>] [--no-accept] [--headless] [--json]
//   defaults: --lang en-US, --tz America/New_York, auto-accepts consent walls
//
//   node net-trace.mjs https://www.google.com/ --cookie "SOCS=CAI;.google.com" --fill "textarea[name=q]=rust lang"
//   node net-trace.mjs https://example.com/ --json > trace.json
//
// A FRESH profile hits consent/cookie walls (e.g. google's "Before you continue"), which pollutes
// the trace with the consent page's requests — seed the consent-suppression cookie with --cookie
// (general; `<name>=<value>` + optional `;<domain>`, default derived from the URL host) to trace
// the real page. --fill runs an interaction (engagement-gated requests fire on interaction); repeatable.
// --json prints the raw {requests, cookieSetters, posts} for machine diffing; default is a
// grouped human report. Needs `patchright` (already a dep here) + a local Chrome channel.
import { chromium } from "patchright";
import { join } from "node:path";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";

function parseArgs(argv) {
  const out = {
    url: null,
    wait: 1500,
    fills: [],
    cookies: [],
    lang: "en-US",
    tz: "America/New_York",
    headless: false,
    json: false,
    accept: true,
  };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--wait") out.wait = parseInt(argv[++i], 10) || out.wait;
    else if (a === "--fill") out.fills.push(argv[++i]);
    else if (a === "--cookie") out.cookies.push(argv[++i]);
    else if (a === "--lang") out.lang = argv[++i] || out.lang;
    else if (a === "--tz") out.tz = argv[++i] || out.tz;
    else if (a === "--headless") out.headless = true;
    else if (a === "--json") out.json = true;
    else if (a === "--no-accept") out.accept = false;
    else if (!a.startsWith("--") && !out.url) out.url = a;
  }
  return out;
}

// Generic consent/cookie-wall accept: find a button whose visible text or aria-label reads like
// an accept-all action and click it, across the main frame and any consent iframes. Page-agnostic
// (works on google's "Before you continue", GDPR banners, etc.) — best-effort, never throws.
async function acceptConsent(page) {
  // Multilingual accept-all matcher — the consent wall renders in the geo/Accept-Language locale
  // (e.g. "Aceitar tudo" in Portugal). Prefer forcing en-US via --lang, but match common langs too.
  const RE =
    /\b(accept all|accept|agree|i agree|allow all|got it|consent|ok, got it|aceitar tudo|aceitar|aceptar todo|aceptar|tout accepter|accepter|alle akzeptieren|akzeptieren|accetta tutto|accetta|alles accepteren|zaakceptuj)\b/i;
  for (const frame of page.frames()) {
    try {
      const handles = await frame.$$("button, [role=button], input[type=submit], a[role=button]");
      for (const h of handles) {
        const label = (
          (await h.getAttribute("aria-label")) ||
          (await h.innerText().catch(() => "")) ||
          ""
        ).trim();
        if (label && RE.test(label)) {
          await h.click({ timeout: 2000 }).catch(() => {});
          await page.waitForTimeout(600);
          return label;
        }
      }
    } catch {}
  }
  return null;
}

const args = parseArgs(process.argv.slice(2));
if (!args.url) {
  process.stderr.write(
    'usage: node net-trace.mjs <url> [--wait ms] [--fill "sel=text"]... [--headless] [--json]\n',
  );
  process.exit(2);
}

// Ephemeral profile per run (gitignored temp dir) so traces are clean + independent.
const profileDir = mkdtempSync(join(tmpdir(), "net-trace-"));
// Force the render/consent language via locale + Accept-Language (default en-US), so a
// geo-localized page (e.g. google in Portugal) renders in the expected language and the
// consent matcher + any text assertions are stable regardless of exit-IP country.
const context = await chromium.launchPersistentContext(profileDir, {
  channel: "chrome",
  headless: args.headless,
  viewport: null,
  locale: args.lang,
  timezoneId: args.tz,
  extraHTTPHeaders: { "Accept-Language": `${args.lang},${args.lang.split("-")[0]};q=0.9` },
});
// Seed any consent-suppression / prefs cookies before the first load (general; domain defaults
// to the URL host). e.g. --cookie "SOCS=CAI;.google.com".
if (args.cookies.length) {
  const host = (() => {
    try {
      return new URL(args.url).hostname;
    } catch {
      return "";
    }
  })();
  const toAdd = args.cookies.map((c) => {
    const [nv, dom] = c.split(";");
    const eq = nv.indexOf("=");
    return {
      name: nv.slice(0, eq).trim(),
      value: nv.slice(eq + 1).trim(),
      domain: (dom || host).trim(),
      path: "/",
    };
  });
  await context
    .addCookies(toAdd)
    .catch((e) => process.stderr.write(`addCookies warning: ${e.message}\n`));
}

const page = context.pages()[0] || (await context.newPage());

const requests = [];
page.on("request", (r) =>
  requests.push({
    method: r.method(),
    url: r.url(),
    type: r.resourceType(),
    // POST body (token/beacon payloads) — capped so a huge upload doesn't bloat the trace.
    postData: r.method() === "POST" ? (r.postData() || "").slice(0, 4000) : null,
  }),
);
const cookieSetters = [];
page.on("response", async (r) => {
  try {
    const h = await r.allHeaders();
    if (h["set-cookie"]) {
      const names = h["set-cookie"].split("\n").map((l) => l.split("=")[0].trim());
      cookieSetters.push({ url: r.url(), status: r.status(), cookies: names });
    }
  } catch {}
});

try {
  await page.goto(args.url, { waitUntil: "networkidle", timeout: 45000 });
} catch (e) {
  process.stderr.write(`goto warning: ${e.message}\n`);
}
// Dismiss a consent/cookie wall so the trace reflects the real page, not the consent flow.
if (args.accept) {
  const clicked = await acceptConsent(page);
  if (clicked) {
    process.stderr.write(`accepted consent: "${clicked}"\n`);
    // Keep capturing through the post-accept redirect + homepage load (that response is what
    // sets the real cookies) — don't clear; the consent-flow requests are minor noise.
    try {
      await page.waitForLoadState("networkidle", { timeout: 15000 });
    } catch {}
  }
}
for (const f of args.fills) {
  // Split on the LAST "=" so a CSS attribute selector (textarea[name=q]) survives; the
  // fill text is assumed free of "=" (fine for search queries + typical inputs).
  const eq = f.lastIndexOf("=");
  const sel = f.slice(0, eq);
  const text = f.slice(eq + 1);
  try {
    await page.fill(sel, text, { timeout: 5000 });
    await page.waitForTimeout(500);
  } catch (e) {
    process.stderr.write(`fill "${sel}" warning: ${e.message}\n`);
  }
}
await page.waitForTimeout(args.wait);

const norm = (u) => {
  try {
    const x = new URL(u);
    return x.origin + x.pathname;
  } catch {
    return u;
  }
};

if (args.json) {
  process.stdout.write(JSON.stringify({ url: args.url, requests, cookieSetters }, null, 2));
} else {
  const byKey = new Map();
  for (const r of requests) {
    const key = `${r.method} ${norm(r.url)} [${r.type}]`;
    byKey.set(key, (byKey.get(key) || 0) + 1);
  }
  const rows = [...byKey.entries()].sort((a, b) => a[0].localeCompare(b[0]));
  process.stdout.write(`=== network cascade on ${args.url} (grouped by method+path) ===\n`);
  for (const [k, n] of rows) process.stdout.write(`${String(n).padStart(3)}x  ${k}\n`);
  process.stdout.write(`\n=== responses that SET a cookie ===\n`);
  if (!cookieSetters.length) process.stdout.write("  (none)\n");
  for (const c of cookieSetters)
    process.stdout.write(`  ${c.status}  ${c.cookies.join(",")}  <-  ${norm(c.url)}\n`);
  process.stdout.write(`\n=== POSTs (with body preview) ===\n`);
  const posts = requests.filter((r) => r.method === "POST");
  if (!posts.length) process.stdout.write("  (none)\n");
  for (const r of posts) {
    process.stdout.write(`  ${norm(r.url)}\n`);
    if (r.postData)
      process.stdout.write(`      body: ${r.postData.slice(0, 300).replace(/\n/g, " ")}\n`);
  }
  // Beacon/attestation endpoints: dump the FULL URL (query carries the token params for GET-style
  // beacons like /gen_204) so the token shape is visible.
  process.stdout.write(
    `\n=== beacon URLs (full query) — gen_204 / client_204 / log / batchexecute ===\n`,
  );
  const beacons = requests.filter((r) =>
    /\/(gen_204|client_204|log|batchexecute|async\/)/.test(r.url),
  );
  if (!beacons.length) process.stdout.write("  (none)\n");
  for (const r of beacons) process.stdout.write(`  ${r.method} ${r.url.slice(0, 500)}\n`);
  process.stdout.write(`\ntotal requests: ${requests.length}\n`);
}

await context.close();
process.exit(0);
