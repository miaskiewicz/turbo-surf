// Generic Incapsula-aware fetcher reusing the hardened-Chrome sidecar profile.
// stdin: {"urls":["...","..."]}  stdout: [{url,status,ok,html}]
import { chromium } from "patchright";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
const HERE = dirname(fileURLToPath(import.meta.url));

function readStdin() {
  return new Promise((resolve) => {
    let buf = "";
    process.stdin.setEncoding("utf8");
    process.stdin.on("data", (d) => (buf += d));
    process.stdin.on("end", () => resolve(buf));
    if (process.stdin.isTTY) resolve("");
  });
}

const isChallenge = (h) => {
  const l = (h || "").toLowerCase();
  return (
    l.includes("_incapsula_resource") ||
    l.includes("incapsula incident") ||
    l.includes("request unsuccessful") ||
    (l.length < 2000 && l.includes("robots") && l.includes("noindex"))
  );
};

async function fetchOne(page, url) {
  let resp = await page.goto(url, { waitUntil: "domcontentloaded", timeout: 45000 });
  // Let the Incapsula JS challenge execute + auto-reload, then re-check a few times.
  for (let attempt = 0; attempt < 5; attempt++) {
    await page.waitForTimeout(4000);
    let html = await page.content();
    if (!isChallenge(html)) {
      // Give the SPA a moment to render the tesis body.
      await page.waitForTimeout(2500).catch(() => {});
      html = await page.content();
      return { url, status: resp ? resp.status() : 0, ok: !isChallenge(html), html };
    }
    // Still challenged — try a reload (cookie may now be set) and wait for network idle.
    try {
      resp = await page.reload({ waitUntil: "networkidle", timeout: 45000 });
    } catch {
      /* keep looping */
    }
  }
  const html = await page.content();
  return { url, status: resp ? resp.status() : 0, ok: !isChallenge(html), html };
}

async function main() {
  const req = JSON.parse((await readStdin()) || "{}");
  const urls = req.urls || (req.url ? [req.url] : []);
  const userDataDir = join(HERE, ".chrome-profile");
  const context = await chromium.launchPersistentContext(userDataDir, {
    channel: "chrome",
    headless: true,
    viewport: null,
    locale: "es-MX",
    timezoneId: "America/Mexico_City",
  });
  const out = [];
  try {
    const page = context.pages()[0] || (await context.newPage());
    for (const url of urls) {
      try {
        out.push(await fetchOne(page, url));
      } catch (e) {
        out.push({ url, status: 0, ok: false, html: "", error: String(e && e.message ? e.message : e) });
      }
    }
  } finally {
    await context.close();
  }
  process.stdout.write(JSON.stringify(out));
}

main().catch((e) => {
  process.stderr.write("fetch-incapsula: " + (e && e.stack ? e.stack : e) + "\n");
  process.exit(1);
});
