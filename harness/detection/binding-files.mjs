// The binding's probe/control files ship inside the turbo-dom-browser-env crate
// package (under `src/`); surf keeps no copies. Locate the package cargo resolved
// for this workspace (registry, git or path) through `cargo metadata`.
import { execFileSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

let source;

/** `file:` URL (with a trailing slash) of the resolved turbo-dom-browser-env `src/`. */
export function bindingSource() {
  if (source) return source;
  const manifest = fileURLToPath(new URL("../../rust/Cargo.toml", import.meta.url));
  const metadata = JSON.parse(
    execFileSync(
      process.env.CARGO ?? "cargo",
      ["metadata", "--format-version", "1", "--manifest-path", manifest],
      { encoding: "utf8", maxBuffer: 1 << 30, stdio: ["ignore", "pipe", "pipe"] },
    ),
  );
  const binding = metadata.packages.filter((entry) => entry.name === "turbo-dom-browser-env");
  if (binding.length !== 1)
    throw new Error(`expected one turbo-dom-browser-env package, found ${binding.length}`);
  source = pathToFileURL(join(dirname(binding[0].manifest_path), "src") + "/").href;
  return source;
}
