import { execFile } from "node:child_process";
import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import { chmod, lstat, mkdir, readdir, readlink, realpath, writeFile } from "node:fs/promises";
import { basename, dirname, isAbsolute, join, relative, resolve, sep } from "node:path";
import { promisify } from "node:util";
import { fileURLToPath } from "node:url";

const execute = promisify(execFile);
const sha = (value) => createHash("sha256").update(value).digest("hex");

export async function snapshotBundle(directory) {
  if (!(await lstat(directory)).isDirectory())
    throw new Error("Bundle root must be a real directory");
  const root = await realpath(directory);
  const entries = [];
  async function visit(path, name) {
    const info = await lstat(path);
    const mode = info.mode & 0o777;
    if (info.isSymbolicLink()) {
      const target = await readlink(path);
      const actual = relative(root, await realpath(path));
      if (actual === ".." || actual.startsWith(`..${sep}`) || isAbsolute(actual))
        throw new Error(`Bundle symlink escapes its root: ${name}`);
      entries.push({ path: name, type: "symlink", target });
    } else if (info.isDirectory()) {
      entries.push({ path: name, type: "directory", mode });
      for (const child of (await readdir(path)).sort())
        await visit(join(path, child), name ? `${name}/${child}` : child);
    } else if (info.isFile()) {
      const digest = createHash("sha256");
      for await (const chunk of createReadStream(path)) digest.update(chunk);
      const after = await lstat(path);
      if (
        info.size !== after.size ||
        info.mtimeMs !== after.mtimeMs ||
        info.ctimeMs !== after.ctimeMs ||
        info.ino !== after.ino
      )
        throw new Error(`Bundle file changed while hashing: ${name}`);
      entries.push({
        path: name,
        type: "file",
        mode,
        bytes: info.size,
        sha256: digest.digest("hex"),
      });
    } else throw new Error(`Unsupported bundle entry: ${name}`);
  }
  await visit(root, "");
  return { sha256: sha(JSON.stringify(entries)), entries };
}

const copyBundle = async (source, destination) =>
  execute("/usr/bin/ditto", ["--norsrc", "--noextattr", source, destination]);
const verifyBundle = async (path) =>
  execute("/usr/bin/codesign", ["--verify", "--deep", "--strict", path]);

async function canonicalDestination(path) {
  const missing = [];
  let ancestor = path;
  for (;;) {
    try {
      return join(await realpath(ancestor), ...missing);
    } catch (error) {
      if (error.code !== "ENOENT") throw error;
      try {
        await lstat(ancestor);
        throw new Error("Destination ancestor is an unresolved symlink");
      } catch (statError) {
        if (statError.code !== "ENOENT") throw statError;
      }
      missing.unshift(basename(ancestor));
      ancestor = dirname(ancestor);
    }
  }
}

export async function freezeChromeBundle(source, destination, operations = {}) {
  source = await realpath(source);
  destination = resolve(destination);
  const relationship = relative(source, destination);
  if (
    !relationship ||
    (!relationship.startsWith(`..${sep}`) && relationship !== ".." && !isAbsolute(relationship))
  )
    throw new Error("Destination must be outside the source bundle");
  const receiptPath = `${destination}.provenance.json`;
  const canonicalRelationship = relative(source, await canonicalDestination(destination));
  if (
    !canonicalRelationship ||
    (!canonicalRelationship.startsWith(`..${sep}`) &&
      canonicalRelationship !== ".." &&
      !isAbsolute(canonicalRelationship))
  )
    throw new Error("Destination must be outside the source bundle");
  await mkdir(dirname(destination), { recursive: true });
  // Exclusive reservation prevents a repeated invocation from overwriting an oracle.
  await mkdir(destination);
  await chmod(destination, (await lstat(source)).mode & 0o777);
  const receipt = {
    source,
    destination,
    executable: join(destination, "Contents/MacOS/Google Chrome"),
    complete: false,
  };
  try {
    const before = await snapshotBundle(source);
    receipt.sourceSha256Before = before.sha256;
    // Finder metadata is outside the content manifest and can invalidate macOS
    // verification. Verify the copied code/resources without modifying the app.
    await (operations.copy || copyBundle)(source, destination);
    await (operations.verify || verifyBundle)(destination);
    const after = await snapshotBundle(source);
    const copied = await snapshotBundle(destination);
    receipt.sourceSha256After = after.sha256;
    receipt.snapshot = copied;
    if (before.sha256 !== after.sha256) throw new Error("Source bundle changed during copying");
    if (before.sha256 !== copied.sha256) throw new Error("Copied bundle differs from its source");
    receipt.complete = true;
  } catch (error) {
    receipt.error = String(error);
    throw error;
  } finally {
    // Preserve partial copies and failures for diagnosis; never clean an unrelated app.
    await writeFile(receiptPath, `${JSON.stringify(receipt, null, 2)}\n`, {
      flag: "wx",
      mode: 0o600,
    });
  }
  return receipt;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.platform !== "darwin") throw new Error("Chrome app-bundle freezing requires macOS");
  if (process.argv.length !== 4)
    throw new Error("Usage: node freeze-chrome.mjs SOURCE.app DESTINATION.app");
  const receipt = await freezeChromeBundle(process.argv[2], process.argv[3]);
  console.log(
    JSON.stringify({
      executable: receipt.executable,
      bundleSha256: receipt.snapshot.sha256,
      complete: receipt.complete,
    }),
  );
}
