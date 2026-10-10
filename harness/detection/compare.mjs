import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { createHash } from "node:crypto";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

export const endpoint = (row) => {
  try {
    const u = new URL(row.url);
    return `${row.method || "GET"} ${u.origin}${u.pathname}`;
  } catch {
    return `${row.method || "GET"} ${row.url}`;
  }
};
const hash = (value) => createHash("sha256").update(value).digest("hex");
const canonical = (name) =>
  name
    .replace(/\.prototype\./g, ".")
    .replace(/^(HTMLDocument|Document)\./, "document.")
    .replace(/^Navigator\./, "navigator.")
    .replace(/^Screen\./, "screen.")
    .replace(/^Performance\./, "performance.");
const observations = (pages) => {
  const result = new Map();
  for (const page of pages || [])
    for (const row of page.interfaces?.accesses || []) {
      const key = `${canonical(row.name)}:${row.kind}`;
      const old = result.get(key) || { count: 0, values: new Map(), stacks: new Set() };
      // Chrome snapshots are cumulative within a document; maxima avoid counting
      // repeated snapshots as new accesses. This is a coverage comparison, not a timeline.
      old.count = Math.max(old.count, row.count);
      for (const sample of [
        { args: row.args, result: row.result, error: row.error },
        ...(row.samples || []),
      ])
        old.values.set(JSON.stringify(sample), sample);
      if (row.stack) old.stacks.add(row.stack);
      result.set(key, old);
    }
  return result;
};
export const interfaceDiff = (chromePages, nativePages) => {
  const c = observations(chromePages),
    n = observations(nativePages);
  return [...new Set([...c.keys(), ...n.keys()])].sort().map((name) => {
    const chrome = c.get(name),
      native = n.get(name);
    return {
      name,
      chromeCountMax: chrome?.count || 0,
      nativeCountMax: native?.count || 0,
      chromeOnlyValues: [...(chrome?.values || [])]
        .filter(([k]) => !native?.values.has(k))
        .map(([, v]) => v),
      nativeOnlyValues: [...(native?.values || [])]
        .filter(([k]) => !chrome?.values.has(k))
        .map(([, v]) => v),
      chromeStacks: [...(chrome?.stacks || [])],
      nativeStacks: [...(native?.stacks || [])],
    };
  });
};
export const requestSummary = (row) => {
  let queryKeys = [];
  try {
    queryKeys = [...new Set(new URL(row.url).searchParams.keys())].sort();
  } catch {}
  const body = row.body;
  let bodyKeys = [];
  if (typeof body === "string" && body.length) {
    try {
      bodyKeys = Object.keys(JSON.parse(body));
    } catch {
      if (body.includes("=")) bodyKeys = [...new Set(new URLSearchParams(body).keys())];
    }
  }
  return {
    id: row.id,
    url: row.url,
    queryKeys,
    bodyKeys: bodyKeys.sort(),
    bodyBytes: typeof body === "string" ? Buffer.byteLength(body) : 0,
    bodyHash: typeof body === "string" ? hash(body) : null,
    headers: row.headers || row.explicit_headers || {},
    headerScope:
      row.headerScope ||
      (row.explicit_headers
        ? "explicit overrides; backend defaults absent"
        : "browser-reported headers"),
  };
};
export const compare = (chrome, native, cRequests = [], nRequests = []) => {
  const counts = (rows) => {
    const result = new Map();
    for (const row of rows) {
      const key = endpoint(row);
      const entry = result.get(key) || [];
      entry.push(requestSummary(row));
      result.set(key, entry);
    }
    return result;
  };
  const c = counts(cRequests),
    n = counts(nRequests);
  return {
    network: [...new Set([...c.keys(), ...n.keys()])].sort().map((key) => ({
      endpoint: key,
      chrome: c.get(key)?.length || 0,
      native: n.get(key)?.length || 0,
      chromeRequests: c.get(key) || [],
      nativeRequests: n.get(key) || [],
    })),
    interfaces: interfaceDiff(chrome.pages, native.pages),
    features: native.features,
    probes: { chrome: chrome.probes, native: native.probes },
    errors: { chrome: chrome.errors || [], native: native.errors || [] },
    pages: Object.fromEntries(
      [
        ["chrome", chrome],
        ["native", native],
      ].map(([runtime, trace]) => [
        runtime,
        (trace.pages || []).map((p) => ({
          phase: p.phase,
          url: p.url,
          bytes: p.bytes,
          results: p.results,
          status: p.status,
          error: p.error,
          cookies: (p.cookies || []).map((c) => c.name),
        })),
      ]),
    ),
  };
};
export const compareDirectory = (out) => {
  const read = (path, fallback) =>
    existsSync(path) ? JSON.parse(readFileSync(path, "utf8")) : fallback;
  const result = {};
  for (const pass of ["baseline", "interfaces"]) {
    const cdir = join(out, `chrome-${pass}`),
      ndir = join(out, `native-${pass}`);
    const chrome = read(join(cdir, "trace.json"), {
      errors: ["Missing Chrome trace.json; check process logs"],
    });
    const native = read(join(ndir, "trace.json"), {
      errors: ["Missing native trace.json; check process logs"],
    });
    const netpath = join(ndir, "network.jsonl");
    const rows = existsSync(netpath)
      ? readFileSync(netpath, "utf8")
          .trim()
          .split("\n")
          .filter(Boolean)
          .map((l) => {
            try {
              return JSON.parse(l);
            } catch {
              return { event: "incomplete-record" };
            }
          })
      : [];
    result[pass] = compare(
      chrome,
      native,
      chrome.requests,
      rows.filter((r) => r.event === "request"),
    );
    result[pass].traceCoverage = {
      nativeNetworkRecords: rows.length,
      incompleteNativeNetworkRecords: rows.filter((r) => r.event === "incomplete-record").length,
    };
  }
  result.isolatedProbes = Object.fromEntries(
    ["chrome", "native"].map((r) => [
      r,
      read(join(out, `${r}-probes`, "trace.json"), {}).probes || [],
    ]),
  );
  writeFileSync(join(out, "comparison.json"), JSON.stringify(result, null, 2), { mode: 0o600 });
  return result;
};
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const result = compareDirectory(resolve(process.argv[2]));
  for (const [pass, diff] of Object.entries(result).filter(([p]) => p !== "isolatedProbes")) {
    console.log(
      `${pass}: ${diff.network.filter((r) => !r.native).length} endpoints absent from native; ${diff.interfaces.length} observed interfaces`,
    );
    for (const runtime of ["chrome", "native"]) {
      const final = diff.pages[runtime].at(-1);
      console.log(
        `  ${runtime}: ${final?.results || 0} result markers; ${final?.error ? "runtime error" : new URL(final?.url || "about:blank").pathname}`,
      );
    }
  }
}
