import importlib.util
import tomllib
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("release_guard", ROOT / "release_guard.py")
guard = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(guard)

GIT = "git+https://github.com/miaskiewicz/turbo-dom?rev=abc#abc"
MANIFEST = '[workspace]\nmembers = ["crates/turbo-surf-core"]\n\n[profile.release]\nlto = "thin"\n'


def lock(overrides=None, extra=()):
    versions = {name: "0.6.0" for name in guard.TURBO_DOM}
    versions["turbo-dom-v8"] = "150.4.0"
    versions.update({name: "0.1.1" for name in guard.DENO_FAMILY})
    packages = [{"name": "turbo-surf-core", "version": "0.6.0"}]
    for name, version in versions.items():
        package = {"name": name, "version": version, "source": guard.REGISTRY}
        package.update((overrides or {}).get(name, {}))
        packages.append(package)
    packages.extend(extra)
    return {"package": packages}


def published(name):
    return {"0.6.0": False, "150.4.0": False, "0.1.1": False}


class ReleaseGuard(unittest.TestCase):
    def test_registry_only_tree_passes(self):
        self.assertEqual(guard.problems(MANIFEST, lock(), published), [])

    def test_patch_table_and_unused_patch_are_rejected(self):
        manifest = MANIFEST + '\n[patch.crates-io]\nturbo-dom = { git = "x", rev = "y" }\n'
        state = lock()
        state["patch"] = {"unused": [{"name": "turbo-surf-deno-bench-util", "version": "0.253.1"}]}
        found = guard.problems(manifest, state, published)
        self.assertIn("rust/Cargo.toml still has a patch table: [patch.crates-io]", found)
        self.assertIn("rust/Cargo.lock still records an unused patch: turbo-surf-deno-bench-util 0.253.1", found)

    def test_git_sources_are_rejected_anywhere_in_the_lock(self):
        state = lock({"turbo-dom-layout": {"source": GIT}}, [{"name": "other", "version": "1.0.0", "source": GIT}])
        found = guard.problems(MANIFEST, state, published)
        self.assertIn(f"turbo-dom-layout 0.6.0 is locked from {GIT}", found)
        self.assertIn(f"turbo-dom-layout 0.6.0 must come from crates.io, not {GIT}", found)
        self.assertIn(f"other 1.0.0 is locked from {GIT}", found)

    def test_missing_unpublished_and_yanked_prerequisites_are_rejected(self):
        state = lock()
        state["package"] = [p for p in state["package"] if p["name"] != "turbo-surf-deno-web"]

        def registry(name):
            if name == "turbo-dom-raster":
                return {}
            if name == "turbo-surf-deno-core":
                return {"0.1.1": True}
            return published(name)

        found = guard.problems(MANIFEST, state, registry)
        self.assertEqual(
            found,
            [
                "turbo-surf-deno-web is not in rust/Cargo.lock",
                "turbo-dom-raster 0.6.0 is not on crates.io (publish it first)",
                "turbo-surf-deno-core 0.1.1 is yanked on crates.io",
            ],
        )

    def test_duplicate_prerequisite_versions_are_rejected(self):
        state = lock(extra=[{"name": "turbo-dom", "version": "0.5.8", "source": guard.REGISTRY}])
        found = guard.problems(MANIFEST, state, lambda name: {"0.6.0": False, "0.5.8": False, "0.1.1": False, "150.4.0": False})
        self.assertEqual(found, ["rust/Cargo.lock must contain exactly one turbo-dom, found 0.6.0, 0.5.8"])

    def test_offline_lookup_skips_only_the_registry(self):
        state = lock({"turbo-dom": {"source": GIT}})
        found = guard.problems(MANIFEST, state, lambda name: None)
        self.assertEqual(len(found), 2)
        self.assertTrue(all("turbo-dom 0.6.0" in problem for problem in found))

    def test_sparse_index_keys(self):
        self.assertEqual(guard.index_key("a"), "1/a")
        self.assertEqual(guard.index_key("ab"), "2/ab")
        self.assertEqual(guard.index_key("abc"), "3/a/abc")
        self.assertEqual(guard.index_key("turbo-dom"), "tu/rb/turbo-dom")

    def test_the_pre_publication_tree_is_not_releasable_yet(self):
        # The consolidation branch still patches the unpublished crates from Git.
        rust = ROOT.parent
        found = guard.problems(
            (rust / "Cargo.toml").read_text(), tomllib.loads((rust / "Cargo.lock").read_text()), lambda name: None
        )
        if "[patch.crates-io]" in (rust / "Cargo.toml").read_text():
            self.assertIn("rust/Cargo.toml still has a patch table: [patch.crates-io]", found)
        else:
            self.assertEqual(found, [])


if __name__ == "__main__":
    unittest.main()
