import assert from "node:assert/strict";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  realpathSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { basename, dirname, join, relative, resolve } from "node:path";
import test from "node:test";
import { createTestOutput } from "./test-output.mjs";

function withProject(project, run, name) {
  const previous = [process.env.TURBO_PW_PROJECT, process.env.TURBO_PW_PROJECT_NAME];
  try {
    if (project === undefined) delete process.env.TURBO_PW_PROJECT;
    else process.env.TURBO_PW_PROJECT = JSON.stringify(project);
    if (name === undefined) delete process.env.TURBO_PW_PROJECT_NAME;
    else process.env.TURBO_PW_PROJECT_NAME = name;
    return run();
  } finally {
    for (const [index, key] of ["TURBO_PW_PROJECT", "TURBO_PW_PROJECT_NAME"].entries()) {
      if (previous[index] === undefined) delete process.env[key];
      else process.env[key] = previous[index];
    }
  }
}

function withDirectory(run) {
  const directory = realpathSync(mkdtempSync(join(tmpdir(), "surf-output-unit-")));
  try {
    return run(directory);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
}

function configured(directory, run, attempt = {}) {
  return withProject(
    { name: "project", testDir: directory, outputDir: join(directory, "results") },
    () =>
      run(
        createTestOutput(
          { file: join(directory, "example.spec.mjs"), id: "test-id", retry: 0, ...attempt },
          ["suite", "test"],
        ),
      ),
  );
}

test("output metadata outside a test has defaults but cannot allocate artifacts", () => {
  withProject(undefined, () => {
    const titles = ["worker setup"];
    const output = createTestOutput(undefined, titles);
    assert.deepEqual(output.project, {
      name: "turbo-surf",
      testDir: process.cwd(),
      outputDir: resolve("test-results"),
    });
    assert.equal(output.file, undefined);
    assert.deepEqual(output.titlePath, titles);
    assert.throws(() => output.outputDir, /active test attempt with a real file/);
    assert.throws(() => output.outputPath("setup.json"), /active test attempt with a real file/);
  });
});

test("configured metadata creates nested artifact parents lazily without overwriting files", () => {
  withDirectory((directory) => {
    configured(directory, (output) => {
      assert.equal(output.project.name, "project");
      assert.equal(output.file, join(directory, "example.spec.mjs"));
      assert.deepEqual(output.titlePath, ["example.spec.mjs", "suite", "test"]);
      assert.equal(existsSync(output.project.outputDir), false);
      assert.equal(existsSync(output.outputDir), false);
      const artifact = output.outputPath("screenshots", "nested", "failure.txt");
      assert.equal(dirname(dirname(dirname(artifact))), output.outputDir);
      writeFileSync(artifact, "retained screenshot diagnosis");
      assert.equal(output.outputPath("screenshots", "nested", "failure.txt"), artifact);
      assert.equal(output.outputPath(artifact), artifact);
      assert.equal(output.outputPath(), output.outputDir);
      assert.equal(readFileSync(artifact, "utf8"), "retained screenshot diagnosis");
    });
  });
});

test("attempt identity retains artifacts across retries, same basenames and sanitized project names", () => {
  withDirectory((directory) => {
    const variants = [
      { file: join(directory, "left", "same.spec.mjs"), id: "test", retry: 0, project: "alpha/a" },
      { file: join(directory, "right", "same.spec.mjs"), id: "test", retry: 0, project: "alpha/a" },
      { file: join(directory, "left", "same.spec.mjs"), id: "test", retry: 1, project: "alpha/a" },
      { file: join(directory, "left", "same.spec.mjs"), id: "other", retry: 0, project: "alpha/a" },
      { file: join(directory, "left", "same.spec.mjs"), id: "test", retry: 0, project: "alpha-a" },
    ];
    const paths = variants.map(({ project, ...attempt }, index) =>
      withProject(
        { name: project, testDir: directory, outputDir: join(directory, "results") },
        () => {
          const output = createTestOutput(attempt, ["same title"]);
          assert.equal(relative(directory, attempt.file), output.titlePath[0]);
          const artifact = output.outputPath("failure.json");
          writeFileSync(artifact, JSON.stringify({ index }));
          assert.equal(createTestOutput(attempt, ["same title"]).outputDir, output.outputDir);
          return artifact;
        },
      ),
    );
    assert.equal(new Set(paths).size, variants.length);
    paths.forEach((path, index) =>
      assert.deepEqual(JSON.parse(readFileSync(path, "utf8")), { index }),
    );
  });
});

test("environment project names and long unsafe titles become bounded filesystem labels", () => {
  withDirectory((directory) => {
    withProject(
      { testDir: directory, outputDir: join(directory, "results") },
      () => {
        const output = createTestOutput({ file: join(directory, "example.spec.mjs"), retry: 0 }, [
          "nested / title ? ".repeat(20),
        ]);
        assert.equal(output.project.name, "environment name");
        assert.match(basename(output.outputDir), /^test-[a-zA-Z0-9_-]{96}-[a-f0-9]{32}-retry0$/);
        assert.equal(dirname(output.outputDir), join(directory, "results"));
      },
      "environment name",
    );
  });
});

test("escaping paths fail before creating any output directories", () => {
  withDirectory((directory) => {
    configured(directory, (output) => {
      for (const segments of [
        [".."],
        ["..", "escape.txt"],
        [directory, "absolute.txt"],
        [output.outputDir + "-sibling", "escape.txt"],
      ]) {
        assert.throws(() => output.outputPath(...segments), /must stay inside/);
        assert.equal(existsSync(output.project.outputDir), false);
      }
    });
    assert.deepEqual(readdirSync(directory), []);
  });
});

test("an attempt directory replaced by a symlink cannot redirect artifact writes", () => {
  withDirectory((directory) => {
    configured(directory, (output) => {
      const outside = join(directory, "outside");
      mkdirSync(outside);
      mkdirSync(output.project.outputDir);
      symlinkSync(outside, output.outputDir, process.platform === "win32" ? "junction" : "dir");
      assert.throws(() => output.outputPath("failure.txt"), /symbolic link/);
      assert.deepEqual(readdirSync(outside), []);
    });
  });
});

test("nested and final symlinks are rejected including dangling artifact links", () => {
  withDirectory((directory) => {
    configured(directory, (output) => {
      const outside = join(directory, "outside");
      mkdirSync(outside);
      const jump = output.outputPath("jump");
      symlinkSync(outside, jump, process.platform === "win32" ? "junction" : "dir");
      assert.throws(() => output.outputPath("jump", "failure.txt"), /symbolic link/);
      const target = join(outside, "untouched.txt");
      writeFileSync(target, "retained");
      symlinkSync(target, output.outputPath("existing.txt"), "file");
      assert.throws(() => output.outputPath("existing.txt"), /symbolic link/);
      symlinkSync(join(outside, "absent.txt"), output.outputPath("dangling.txt"), "file");
      assert.throws(() => output.outputPath("dangling.txt"), /symbolic link/);
      assert.equal(readFileSync(target, "utf8"), "retained");
      assert.deepEqual(readdirSync(outside), ["untouched.txt"]);
    });
  });
});

test("regular files cannot become an attempt directory or artifact parent", () => {
  withDirectory((directory) => {
    configured(directory, (output) => {
      mkdirSync(output.project.outputDir);
      writeFileSync(output.outputDir, "not a directory");
      assert.throws(() => output.outputPath("failure.txt"), /parent is not a directory/);
      rmSync(output.outputDir);
      writeFileSync(output.outputPath("file-parent"), "keep this file");
      assert.throws(
        () => output.outputPath("file-parent", "failure.txt"),
        /parent is not a directory/,
      );
      assert.equal(readFileSync(join(output.outputDir, "file-parent"), "utf8"), "keep this file");
    });
  });
});

test("an explicitly configured root symlink remains host authority", () => {
  withDirectory((directory) => {
    const authorized = join(directory, "authorized-root");
    const configuredRoot = join(directory, "configured-root");
    mkdirSync(authorized);
    symlinkSync(authorized, configuredRoot, process.platform === "win32" ? "junction" : "dir");
    withProject({ testDir: directory, outputDir: configuredRoot }, () => {
      const output = createTestOutput({ file: join(directory, "test.mjs"), retry: 0 }, ["test"]);
      const path = output.outputPath("result.txt");
      writeFileSync(path, "authorized");
      assert.equal(
        readFileSync(join(authorized, basename(output.outputDir), "result.txt"), "utf8"),
        "authorized",
      );
    });
  });
});
