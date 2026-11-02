/* eslint-disable no-await-in-loop -- the five captures are deliberately sequential. */
import assert from "node:assert/strict";
import { execFile, execFileSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";

const here = fileURLToPath(new URL(".", import.meta.url));
const chrome =
  process.env.TURBO_SURF_CHROME_PATH ||
  "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const fixture = JSON.parse(
  readFileSync(join(here, "fixtures/user_activation_gates_chrome156.json"), "utf8"),
);
const { cases } = JSON.parse(readFileSync(join(here, "user-activation-gates-cases.json"), "utf8"));
const escapeIds = cases.map((item) => item.id).filter((id) => id.startsWith("escape-"));
// Only the exact build the control was recorded on can reproduce it row for row.
const chromeBuild = () => {
  if (!existsSync(chrome)) return null;
  const match = /(\d+\.\d+\.\d+\.\d+)/.exec(
    execFileSync(chrome, ["--version"], { encoding: "utf8" }),
  );
  return match && `Chrome/${match[1]}`;
};

test(
  "escape-* activation-gate captures agree run to run and with the Chrome 156 control (turbo-surf#54)",
  {
    skip: chromeBuild() !== fixture.browser && `needs ${fixture.browser} at TURBO_SURF_CHROME_PATH`,
    timeout: 600000,
  },
  async (t) => {
    assert.deepEqual(escapeIds, [
      "escape-modal-no-activation",
      "escape-modal-prevent-no-activation",
      "escape-modal-prevent-with-activation",
      "escape-modal-opened-after-activation",
      "escape-keydown-prevented",
      "escape-two-modals",
      "escape-closedby",
      "escape-popovers",
      "escape-dialog-and-popover",
    ]);
    const expected = fixture.result.filter((row) => escapeIds.includes(row.id));
    assert.equal(expected.length, escapeIds.length);
    const directory = mkdtempSync(join(tmpdir(), "turbo-activation-gates-repeat-"));
    t.after(() => rmSync(directory, { recursive: true, force: true }));
    for (let run = 1; run <= 5; run++) {
      const output = join(directory, `run-${run}.json`);
      await promisify(execFile)(
        process.execPath,
        [
          join(here, "user-activation-gates-chrome.mjs"),
          join(here, "user-activation-gates-cases.json"),
          output,
        ],
        {
          env: {
            ...process.env,
            TURBO_SURF_CHROME_PATH: chrome,
            TURBO_SURF_CASES: escapeIds.join(","),
          },
          timeout: 120000,
        },
      );
      const capture = JSON.parse(readFileSync(output, "utf8"));
      assert.equal(capture.browser, fixture.browser);
      assert.deepEqual(capture.errors, []);
      assert.deepEqual(capture.result, expected, `run ${run} differs from the control`);
    }
  },
);
