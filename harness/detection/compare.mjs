import { createHash } from "node:crypto";
import { existsSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
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
const observations = (pages, origin) => {
  const result = new Map();
  for (const page of pages || [])
    for (const row of page.interfaces?.accesses || []) {
      if ((row.origin || "page") !== origin) continue;
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
export const interfaceDiff = (chromePages, nativePages, origin = "page") => {
  const c = observations(chromePages, origin),
    n = observations(nativePages, origin);
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
export const captureSummary = (trace) => ({
  complete: trace.captureComplete ?? (Array.isArray(trace.pages) && !(trace.errors || []).length),
  hydrationBudgetExceeded: (trace.pages || []).some((page) => page.hydration?.budget_exceeded),
  inputError: (trace.pages || []).find((page) => page.phase === "interaction-error")?.error || null,
  inputState:
    [...(trace.pages || [])].reverse().find((page) => page.inputState)?.inputState || null,
  dismissal: trace.dismissal || null,
});
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
    childInterfaces: interfaceDiff(
      (chrome.pages || []).flatMap((page) => page.childFrames || []),
      (native.pages || []).flatMap((page) => page.childFrames || []),
    ),
    workerInterfaces: interfaceDiff(
      (chrome.pages || []).flatMap((page) => page.workers || []),
      (native.pages || []).flatMap((page) => page.workers || []),
    ),
    runtimeInterfaces: interfaceDiff(chrome.pages, native.pages, "runtime"),
    features: native.features,
    capture: { chrome: captureSummary(chrome), native: captureSummary(native) },
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
          environment: p.environment,
          url: p.url,
          bytes: p.bytes,
          results: p.results,
          status: p.status,
          hydration: p.hydration,
          inputState: p.inputState,
          error: p.error,
          cookies: (p.cookies || []).map((c) => c.name),
        })),
      ]),
    ),
  };
};
export const nativeScriptInventory = (directory) => {
  const root = join(directory, "native-js");
  if (!existsSync(root))
    return { enabled: false, isolates: [], scripts: 0, dropped: 0, pendingSources: 0, errors: [] };
  const isolates = [],
    errors = [];
  for (const entry of readdirSync(root, { withFileTypes: true })) {
    if (!entry.isDirectory() || !/^isolate-\d+$/.test(entry.name)) continue;
    try {
      const inventory = JSON.parse(readFileSync(join(root, entry.name, "inventory.json"), "utf8"));
      isolates.push({ isolate: entry.name, ...inventory });
    } catch (error) {
      errors.push(`${entry.name}: ${error.message}`);
    }
  }
  return {
    enabled: true,
    isolates,
    scripts: isolates.reduce((sum, row) => sum + (row.scripts?.length || 0), 0),
    dropped: isolates.reduce((sum, row) => sum + (row.dropped || 0), 0),
    pendingSources: isolates.reduce((sum, row) => sum + (row.pendingSources || 0), 0),
    errors: [...errors, ...isolates.flatMap((row) => row.errors || [])],
  };
};
export const compareDirectory = (out) => {
  const read = (path, fallback) => {
    if (!existsSync(path)) return fallback;
    try {
      return JSON.parse(readFileSync(path, "utf8"));
    } catch (error) {
      return { ...fallback, errors: [`Incomplete trace.json: ${error.message}`] };
    }
  };
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
      nativeScriptInventory: nativeScriptInventory(ndir),
      chromeCompiledScripts: chrome.scripts?.length || 0,
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
        `  ${runtime}: ${final?.results || 0} result markers; ${final?.error ? "runtime error" : new URL(final?.url || "about:blank").pathname}; capture ${diff.capture[runtime].complete ? "complete" : "incomplete"}${diff.capture[runtime].hydrationBudgetExceeded ? "; hydration budget exhausted" : ""}`,
      );
    }
  }
}
