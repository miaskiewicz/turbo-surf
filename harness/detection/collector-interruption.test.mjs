import assert from "node:assert/strict";
import { test } from "node:test";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  appendFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { superviseChild } from "./child-supervision.mjs";

const chrome =
  process.env.TURBO_SURF_CHROME_PATH ||
  "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const retained = process.env.TURBO_SURF_DETECTION_TEST_ARTIFACTS;
const here = fileURLToPath(new URL(".", import.meta.url));
const sha = (path) => createHash("sha256").update(readFileSync(path)).digest("hex");
const pause = (ms) => new Promise((done) => setTimeout(done, ms));
const version = (text) => text.match(/\d+\.\d+\.\d+\.\d+/)?.[0];
const sameProcess = (a, b) => a?.started === b?.started && a?.command === b?.command;

function processRows() {
  const text = execFileSync("ps", ["-ww", "-axo", "pid=,ppid=,pgid=,lstart=,command="], {
    encoding: "utf8",
    maxBuffer: 8 * 1024 * 1024,
    env: { ...process.env, LC_ALL: "C" },
  });
  return text.split("\n").flatMap((line) => {
    const match = line.match(
      /^\s*(\d+)\s+(\d+)\s+(\d+)\s+(\S+\s+\S+\s+\d+\s+[\d:]+\s+\d+)\s+(.*)$/,
    );
    return match
      ? [
          {
            pid: +match[1],
            parent: +match[2],
            group: +match[3],
            started: match[4],
            command: match[5],
          },
        ]
      : [];
  });
}

function ownedRows(owner, observed) {
  const rows = processRows();
  const leader = rows.find((row) => row.pid === owner.pid);
  const groupOwned =
    !leader || !observed.has(owner.pid) || sameProcess(observed.get(owner.pid), leader);
  const pids = new Set(
    rows.filter((row) => sameProcess(observed.get(row.pid), row)).map((row) => row.pid),
  );
  if (groupOwned) pids.add(owner.pid);
  let changed;
  do {
    changed = false;
    for (const row of rows)
      if ((groupOwned && row.group === owner.pid) || pids.has(row.parent)) {
        if (!pids.has(row.pid)) changed = true;
        pids.add(row.pid);
      }
  } while (changed);
  return rows.filter((row) => pids.has(row.pid));
}

async function waitForStall(t, ready, finished) {
  for (;;) {
    t.signal.throwIfAborted();
    if (finished())
      throw new Error("Collector exited before the controlled stalled-document proof");
    if (ready()) return;
    await pause(20);
  }
}

for (const backend of ["cdp", "patchright"]) {
  test(
    `controlled SIGTERM checkpoints and retires owned ${backend} Chrome`,
    {
      timeout: 45000,
      skip:
        process.platform === "win32" ||
        !existsSync(chrome) ||
        typeof WebSocket !== "function" ||
        (backend === "patchright" &&
          !existsSync(
            new URL(
              "../../scripts/browser-sidecar/node_modules/patchright/package.json",
              import.meta.url,
            ),
          )),
    },
    async (t) => {
      if (retained) mkdirSync(retained, { recursive: true, mode: 0o700 });
      const directory = mkdtempSync(join(retained || tmpdir(), `turbo-interruption-${backend}-`));
      const output = join(directory, "capture");
      mkdirSync(output, { mode: 0o700 });
      const observations = [],
        observed = new Map();
      const controller = new AbortController();
      const signal = AbortSignal.any([t.signal, controller.signal]);
      let stalled = false,
        finished = false,
        passed = false,
        pending,
        owner,
        monitor,
        monitorError,
        result;
      const beforeHash = sha(chrome);
      const beforeVersion = execFileSync(chrome, ["--version"], { encoding: "utf8" }).trim();
      const sample = () => {
        const rows = ownedRows(owner, observed);
        for (const row of rows) observed.set(row.pid, row);
        observations.push({
          at: Date.now(),
          evidencePresent: existsSync(output),
          profilePresent: existsSync(owner.profile),
          rows,
        });
        return rows;
      };
      const server = createServer((request) => {
        if (request.url === "/stalled") stalled = true;
        // No headers/body: navigation is genuinely pending when the host interrupts it.
      });
      try {
        await new Promise((done, fail) => {
          server.once("error", fail);
          server.listen(0, "127.0.0.1", done);
        });
        const config = join(directory, "config.json");
        writeFileSync(
          config,
          JSON.stringify({
            url: `http://127.0.0.1:${server.address().port}/stalled`,
            chromePath: chrome,
            chromeBackend: backend,
            chromeIncognito: backend === "cdp",
            interfaces: false,
            debugger: false,
            steps: [],
            waitMs: 0,
            observationMs: 0,
          }),
          { mode: 0o600 },
        );
        for (const name of ["stdout", "stderr"])
          writeFileSync(join(output, `${name}.log`), "", { mode: 0o600 });
        pending = superviseChild(
          process.execPath,
          [
            "--import",
            join(here, "fixtures/owned-chrome-spawn.mjs"),
            join(here, backend === "cdp" ? "chrome-cdp-trace.mjs" : "chrome-trace.mjs"),
            config,
            output,
          ],
          {
            signal,
            timeoutMs: 45000,
            spawnOptions: {
              env: { ...process.env, TURBO_SURF_BROWSER_CMD: "", TURBO_SURF_BROWSER_FETCH_CMD: "" },
            },
            onOutput: (name, bytes) => appendFileSync(join(output, `${name}.log`), bytes),
          },
        );
        t.after(() => pending);
        pending.then(() => {
          finished = true;
        });
        const ledger = join(output, "owned-process.json");
        await waitForStall(
          t,
          () => stalled && existsSync(ledger),
          () => finished,
        );
        owner = JSON.parse(readFileSync(ledger, "utf8"));
        assert.ok(Number.isInteger(owner.pid) && owner.pid > 0);
        assert.equal(owner.executable, chrome);
        assert.equal(owner.detached, true);
        assert.equal(existsSync(owner.profile), true);
        const initial = sample();
        const leader = initial.find((row) => row.pid === owner.pid);
        assert.ok(leader, "OS independently sees the captured Chrome leader");
        assert.equal(leader.group, owner.pid);
        assert.ok(leader.command.includes(`--user-data-dir=${owner.profile}`));
        observations.push({ at: Date.now(), action: "controlled-abort", stalled });
        controller.abort();
        monitor = setInterval(() => {
          try {
            sample();
          } catch (error) {
            monitorError ??= error;
          }
        }, 50);
        result = await pending;
        clearInterval(monitor);
        assert.equal(monitorError, undefined);
        assert.equal(
          t.signal.aborted,
          false,
          "an outer timeout cannot stand in for the controlled abort",
        );
        assert.equal(result.aborted, true);
        assert.equal(result.timedOut, false);
        assert.equal(result.error, null);
        assert.equal(
          result.signal,
          null,
          "cooperative collector exits before the cleanup-only KILL",
        );
        assert.ok(Number.isInteger(result.code) && result.code !== 0);
        assert.equal(existsSync(output), true);
        assert.equal(
          observations.every((row) => row.evidencePresent !== false),
          true,
        );
        assert.deepEqual(sample(), [], "the owned browser group and observed helpers are gone");
        for (const [pid, prior] of observed) {
          try {
            process.kill(pid, 0);
          } catch (error) {
            if (error.code === "ESRCH") continue;
            throw error;
          }
          assert.equal(
            sameProcess(
              prior,
              processRows().find((row) => row.pid === pid),
            ),
            false,
            "a reused unrelated PID cannot count as a surviving owned process",
          );
        }
        assert.equal(existsSync(owner.profile), false);
        const trace = JSON.parse(readFileSync(join(output, "trace.json"), "utf8"));
        assert.equal(trace.captureComplete, false);
        assert.equal(trace.backend, backend === "cdp" ? "raw-cdp" : "patchright");
        assert.ok(
          trace.collectorErrors.some(
            (error) => error.phase === "interrupted" && error.signal === "SIGTERM",
          ),
        );
        assert.equal(trace.executable, chrome);
        assert.equal(version(trace.browserVersion), version(beforeVersion));
        assert.equal(typeof trace.browserV8Version, "string");
        if (backend === "cdp") {
          assert.equal(trace.browserContext.nonDefaultContext, true);
          assert.equal(trace.browserContext.createdContextVerified, true);
        }
        assert.equal(sha(chrome), beforeHash);
        assert.equal(
          execFileSync(chrome, ["--version"], { encoding: "utf8" }).trim(),
          beforeVersion,
        );
        passed = true;
      } finally {
        controller.abort();
        if (pending) result = await pending;
        clearInterval(monitor);
        // A failed assertion retains the leak proof, then cleans up only recorded identities.
        const fallback = [];
        if (!passed && owner)
          for (const row of ownedRows(owner, observed)) {
            if (!sameProcess(observed.get(row.pid), row)) continue;
            try {
              process.kill(row.pid, "SIGKILL");
              fallback.push(row.pid);
            } catch (error) {
              if (error.code !== "ESRCH") fallback.push({ pid: row.pid, error: String(error) });
            }
          }
        server.closeAllConnections();
        if (server.listening) await new Promise((done) => server.close(done));
        writeFileSync(
          join(directory, "cleanup-proof.json"),
          JSON.stringify(
            {
              passed,
              beforeHash,
              beforeVersion,
              owner,
              result,
              observations,
              fallback,
              profileRemoved: owner ? !existsSync(owner.profile) : null,
            },
            null,
            2,
          ),
          { mode: 0o600 },
        );
        if (passed && !retained) rmSync(directory, { recursive: true, force: true });
        else t.diagnostic(`Chrome interruption evidence retained at ${directory}`);
      }
    },
  );
}
