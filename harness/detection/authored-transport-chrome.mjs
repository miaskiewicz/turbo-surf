// Fresh installed Chrome and the supplied native executable; no cookie sharing.
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { cdpConnection } from "./cdp-connection.mjs";
import { chromeExtensionArgs } from "./chrome-launch-policy.mjs";
import { differences } from "./api-shape.mjs";
import { captureClientHello, normalizeHello } from "./transport-hello.mjs";

const [binaryArgument, outputArgument] = process.argv.slice(2);
if (!binaryArgument || !outputArgument)
  throw new Error("Usage: node authored-transport-chrome.mjs native-executable output-directory");
const binary = resolve(binaryArgument),
  out = resolve(outputArgument);
mkdirSync(out, { recursive: true, mode: 0o700 });
const profile = mkdtempSync(join(out, "chrome-profile-"));
const pause = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds));
const stop = async (child, group = false) => {
  if (!child || child.exitCode !== null || child.signalCode !== null) return;
  const signal = (kind) => {
    try {
      group ? process.kill(-child.pid, kind) : child.kill(kind);
    } catch (error) {
      if (error.code !== "ESRCH") throw error;
    }
  };
  signal("SIGTERM");
  await Promise.race([new Promise((resolve) => child.once("close", resolve)), pause(1000)]);
  if (child.exitCode === null && child.signalCode === null) signal("SIGKILL");
};
let nativeProcess, chromeProcess, connection, socket, version, flags;
try {
  const native = await captureClientHello(
    async (url) => {
      nativeProcess = spawn(binary, [url], { stdio: "ignore" });
      await new Promise((resolve, reject) => {
        nativeProcess.once("spawn", resolve);
        nativeProcess.once("error", reject);
      });
      return () => stop(nativeProcess);
    },
    { out, label: "native" },
  );
  const chrome = await captureClientHello(
    async (url) => {
      flags = [
        "--remote-debugging-port=0",
        `--user-data-dir=${profile}`,
        "--no-first-run",
        "--no-default-browser-check",
        ...chromeExtensionArgs({ url, disableExtensions: true }),
        url,
      ];
      chromeProcess = spawn(
        process.env.TURBO_SURF_CHROME_PATH ||
          "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        flags,
        { detached: true, stdio: ["ignore", "ignore", "pipe"] },
      );
      let stderr = "",
        spawnError;
      chromeProcess.once("error", (error) => {
        spawnError = error;
      });
      chromeProcess.stderr.on("data", (chunk) => {
        if (stderr.length < 1048576) stderr += String(chunk);
      });
      const deadline = Date.now() + 20000;
      let endpoint;
      while (!(endpoint = /DevTools listening on (ws:\/\/[^\s]+)/.exec(stderr)?.[1])) {
        if (spawnError) throw spawnError;
        if (chromeProcess.exitCode !== null || Date.now() > deadline)
          throw new Error("Chrome startup failed");
        await pause(25);
      }
      socket = new WebSocket(endpoint);
      await new Promise((resolve, reject) => {
        socket.onopen = resolve;
        socket.onerror = reject;
      });
      connection = cdpConnection(socket, () => {}, 10000);
      version = await connection.call("Browser.getVersion");
      return async () => {
        connection.close();
        socket.close();
        await stop(chromeProcess, true);
      };
    },
    { out, label: "chrome" },
  );
  const normalized = { native: normalizeHello(native), chrome: normalizeHello(chrome) };
  const result = {
    schemaVersion: 2,
    collectedAt: new Date().toISOString(),
    browser: version.product,
    v8: version.jsVersion,
    flags: flags.map((flag) =>
      flag.startsWith("--user-data-dir=") ? "--user-data-dir=<fresh-profile>" : flag,
    ),
    binarySha256: createHash("sha256").update(readFileSync(binary)).digest("hex"),
    sources: Object.fromEntries(
      ["authored-transport-chrome.mjs", "transport-hello.mjs"].map((name) => [
        name,
        createHash("sha256")
          .update(readFileSync(new URL(name, import.meta.url)))
          .digest("hex"),
      ]),
    ),
    note: "Only authored localhost TLS ClientHello traffic. Raw records preserve exact order, randoms, GREASE, key shares and ECH bytes; normalized metadata keeps capabilities. No cookie sharing or full TLS/H2/browser parity claim.",
    native,
    chrome,
    normalized,
    differences: differences(normalized.chrome, normalized.native),
  };
  writeFileSync(join(out, "comparison.json"), JSON.stringify(result, null, 2) + "\n", {
    mode: 0o600,
  });
  console.log(
    JSON.stringify({ out, browser: result.browser, differences: result.differences.length }),
  );
} finally {
  connection?.close();
  socket?.close();
  await stop(nativeProcess);
  await stop(chromeProcess, true);
  rmSync(profile, { recursive: true, force: true });
}
