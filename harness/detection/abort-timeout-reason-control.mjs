import { readFileSync, writeFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { createRequire } from "node:module";
import { homedir } from "node:os";
import { join } from "node:path";
const require = createRequire(import.meta.url);
const { chromium } = require("playwright-core");
const source = readFileSync(new URL("./abort-timeout-reason-probe.js", import.meta.url), "utf8");
const lifetimeSource = readFileSync(
  new URL("./abort-timeout-reason-lifetime-probe.js", import.meta.url),
  "utf8",
);
let browser;
const deadline = setTimeout(() => {
  console.error("Chrome abort reason oracle exceeded 25s");
  process.exit(124);
}, 25000);
try {
  browser = await chromium.launch({
    executablePath: join(
      homedir(),
      ".cache/turbo-consolidation/chrome156/chrome-headless-shell-mac-arm64/chrome-headless-shell",
    ),
    headless: true,
    args: ["--use-mock-keychain", "--password-store=basic"],
    timeout: 10000,
  });
  const page = await browser.newPage();
  const cdp = await page.context().newCDPSession(page);
  const version = await cdp.send("Browser.getVersion");
  await page.goto("about:blank");
  const started = performance.now();
  const result = await page.evaluate((source) => (0, eval)(source), source);
  for (let index = 0; index < 3; index++) await cdp.send("HeapProfiler.collectGarbage");
  const lifetime = JSON.parse(await page.evaluate((source) => (0, eval)(source), lifetimeSource));
  const report = {
    lifetime,
    lifetimeProbeSha256: createHash("sha256").update(lifetimeSource).digest("hex"),
    version,
    probeSha256: createHash("sha256").update(source).digest("hex"),
    elapsedMs: performance.now() - started,
    result,
  };
  writeFileSync(
    new URL("./fixtures/abort-timeout-reason-chrome156.json", import.meta.url),
    JSON.stringify(report, null, 2) + "\n",
  );
  console.log(JSON.stringify(report));
} finally {
  if (browser) await browser.close();
  clearTimeout(deadline);
}
