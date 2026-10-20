import assert from "node:assert/strict";
import {
  cp,
  chmod,
  lstat,
  mkdir,
  mkdtemp,
  readFile,
  realpath,
  rm,
  symlink,
  writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { freezeChromeBundle, snapshotBundle } from "./freeze-chrome.mjs";

async function fixture(t) {
  const root = await realpath(await mkdtemp(join(tmpdir(), "turbo-freeze-chrome-")));
  t.after(() => rm(root, { recursive: true, force: true }));
  const source = join(root, "Source.app");
  const destination = join(root, "Frozen.app");
  await mkdir(join(source, "Contents"), { recursive: true });
  await writeFile(join(source, "Contents", "fixture"), "version-one");
  await symlink("fixture", join(source, "Contents", "Current"));
  return { root, source, destination };
}
const copy = (source, destination) =>
  cp(source, destination, { recursive: true, verbatimSymlinks: true });
const verify = async () => {};

test("freezes a complete bundle with internal links and a matching manifest", async (t) => {
  const { source, destination } = await fixture(t);
  await chmod(source, 0o775);
  const verified = [];
  const receipt = await freezeChromeBundle(source, destination, {
    copy,
    verify: async (path) => verified.push(path),
  });
  assert.equal(receipt.complete, true);
  assert.deepEqual(verified, [destination]);
  assert.equal(receipt.snapshot.sha256, (await snapshotBundle(source)).sha256);
  assert.equal(JSON.parse(await readFile(`${destination}.provenance.json`)).complete, true);
});

test("refuses an existing destination without changing its files", async (t) => {
  const { source, destination } = await fixture(t);
  await mkdir(destination);
  await writeFile(join(destination, "keep"), "existing");
  await assert.rejects(freezeChromeBundle(source, destination, { copy, verify }), /EEXIST/);
  assert.equal(await readFile(join(destination, "keep"), "utf8"), "existing");
});

test("rejects links to resources outside the bundle", async (t) => {
  const { root, source } = await fixture(t);
  await writeFile(join(root, "outside"), "external");
  await symlink("../../outside", join(source, "Contents", "escape"));
  await assert.rejects(snapshotBundle(source), /escapes/);
});

test("rejects source updates during copying and retains a failure receipt", async (t) => {
  const { source, destination } = await fixture(t);
  await assert.rejects(
    freezeChromeBundle(source, destination, {
      verify,
      copy: async (from, to) => {
        await copy(from, to);
        await writeFile(join(from, "Contents", "fixture"), "version-two");
      },
    }),
    /changed during copying/,
  );
  assert.equal(JSON.parse(await readFile(`${destination}.provenance.json`)).complete, false);
});

test("rejects an incomplete copy", async (t) => {
  const { source, destination } = await fixture(t);
  await assert.rejects(
    freezeChromeBundle(source, destination, { copy: async () => {}, verify }),
    /differs/,
  );
});

test("signature failure preserves an incomplete copy without accepting it", async (t) => {
  const { source, destination } = await fixture(t);
  let copied = false;
  await assert.rejects(
    freezeChromeBundle(source, destination, {
      copy: async () => {
        copied = true;
      },
      verify: async () => {
        throw new Error("signature failed");
      },
    }),
    /signature failed/,
  );
  assert.equal(copied, true);
  assert.equal(JSON.parse(await readFile(`${destination}.provenance.json`)).complete, false);
});

test("refuses nested destinations before creating them", async (t) => {
  const { source } = await fixture(t);
  await assert.rejects(
    freezeChromeBundle(source, join(source, "Nested.app"), { copy, verify }),
    /outside/,
  );
});

test("refuses a destination whose parent link points inside the source", async (t) => {
  const { root, source } = await fixture(t);
  await symlink(source, join(root, "alias"));
  await assert.rejects(
    freezeChromeBundle(source, join(root, "alias", "Nested.app"), { copy, verify }),
    /outside/,
  );
});

test("does not create missing parents through a link into the source", async (t) => {
  const { root, source } = await fixture(t);
  await symlink(source, join(root, "alias"));
  await assert.rejects(
    freezeChromeBundle(source, join(root, "alias", "new-parent", "Frozen.app"), { copy, verify }),
    /outside/,
  );
  await assert.rejects(lstat(join(source, "new-parent")), { code: "ENOENT" });
});
