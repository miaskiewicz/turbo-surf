import { execFileSync } from "node:child_process";
import test from "node:test";
import { fileURLToPath } from "node:url";

test("experimental SDK workspaces preserve production Cargo locks and current sources", () => {
  execFileSync(
    "python3",
    [fileURLToPath(new URL("./engine-154/test_surf_workspace.py", import.meta.url))],
    { stdio: "pipe", timeout: 30000 },
  );
});
