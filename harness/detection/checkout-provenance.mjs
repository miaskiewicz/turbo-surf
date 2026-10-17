import { spawn, execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { createReadStream, lstatSync, readlinkSync } from "node:fs";
import { join } from "node:path";

const hashFile = (path) =>
  new Promise((resolve, reject) => {
    const hash = createHash("sha256"),
      input = createReadStream(path);
    input.on("data", (chunk) => hash.update(chunk));
    input.on("error", reject);
    input.on("end", () => resolve(hash.digest("hex")));
  });

export async function checkoutProvenance(root) {
  // A vendored source replacement can exceed execFileSync's default buffer.
  // Hash the complete diff as a stream; never persist its source contents.
  const dirtyDiffHash = await new Promise((resolve, reject) => {
    const hash = createHash("sha256");
    const process = spawn("git", ["diff", "--no-ext-diff", "HEAD"], { cwd: root });
    let error = "";
    process.stdout.on("data", (chunk) => hash.update(chunk));
    process.stderr.on("data", (chunk) => {
      error = (error + chunk).slice(-2048);
    });
    process.on("error", reject);
    process.on("close", (code) =>
      code === 0
        ? resolve(hash.digest("hex"))
        : reject(new Error(`git diff failed (${code}): ${error}`)),
    );
  });
  const paths = execFileSync("git", ["ls-files", "--others", "--exclude-standard", "-z"], {
    cwd: root,
    encoding: "utf8",
    maxBuffer: 8 * 1024 * 1024,
  })
    .split("\0")
    .filter(Boolean)
    .sort();
  const untrackedHashes = {};
  for (const path of paths) {
    const absolute = join(root, path);
    // Record symlinks themselves, without reading a target outside the checkout.
    untrackedHashes[path] = lstatSync(absolute).isSymbolicLink()
      ? createHash("sha256").update(readlinkSync(absolute)).digest("hex")
      : await hashFile(absolute);
  }
  return { dirtyDiffHash, untrackedHashes };
}
