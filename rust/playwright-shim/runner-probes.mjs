import { spawnSync } from "node:child_process";
import { mkdtemp, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

// Keep intentional failing runner contracts out of the ordinary E2E file sweep.
export async function runShimProbe(body, options = {}) {
  const root = await mkdtemp(join(tmpdir(), "surf-runner-probe-"));
  const file = join(root, "probe.mjs");
  const shim = new URL("./index.mjs", import.meta.url).href;
  await writeFile(
    file,
    `import assert from 'node:assert/strict';\nimport {test as base,test,expect} from ${JSON.stringify(shim)};\n${body}`,
  );
  const env = { ...process.env, ...options.env };
  delete env.NODE_TEST_CONTEXT;
  return spawnSync(process.execPath, ["--test", "--test-force-exit", file], {
    env,
    encoding: "utf8",
    timeout: options.timeout ?? 10000,
  });
}
