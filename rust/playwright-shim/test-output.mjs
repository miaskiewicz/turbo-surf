import { createHash } from "node:crypto";
import { lstatSync, mkdirSync, realpathSync } from "node:fs";
import { basename, dirname, isAbsolute, join, relative, resolve, sep } from "node:path";

function inspect(path) {
  const entry = lstatSync(path, { throwIfNoEntry: false });
  if (entry?.isSymbolicLink()) throw new Error("Test output paths cannot traverse a symbolic link");
  return entry;
}

function ensureDirectory(path) {
  const entry = inspect(path);
  if (!entry) mkdirSync(path);
  else if (!entry.isDirectory()) throw new Error("Test output parent is not a directory");
}

function containedPath(root, directory, segments) {
  const target = resolve(directory, ...segments);
  const suffix = relative(directory, target);
  if (suffix === ".." || suffix.startsWith(`..${sep}`) || isAbsolute(suffix))
    throw new Error("outputPath must stay inside the test output directory");
  // The configured root is explicit host authority; each attempt's descendants are owned.
  mkdirSync(root, { recursive: true });
  ensureDirectory(directory);
  const parents = target === directory ? "" : relative(directory, dirname(target));
  let parent = directory;
  for (const part of parents.split(sep).filter(Boolean)) {
    parent = join(parent, part);
    ensureDirectory(parent);
  }
  inspect(target);
  return target;
}

export function createTestOutput(attempt, titles) {
  const project = {
    name: process.env.TURBO_PW_PROJECT_NAME ?? "turbo-surf",
    testDir: process.cwd(),
    outputDir: resolve("test-results"),
    ...JSON.parse(process.env.TURBO_PW_PROJECT ?? "{}"),
  };
  const file = attempt?.file;
  const titlePath = file ? [relative(realpathSync(project.testDir), file), ...titles] : titles;
  const label = [basename(file ?? ""), ...titles, project.name]
    .join("-")
    .replace(/[^a-zA-Z0-9_-]+/g, "-")
    .slice(0, 96);
  const identity = JSON.stringify([file, attempt?.id, project.name, attempt?.retry ?? 0]);
  const hash = createHash("sha256").update(identity).digest("hex").slice(0, 32);
  const directory = file
    ? join(project.outputDir, `test-${label}-${hash}-retry${attempt.retry}`)
    : undefined;
  const requireDirectory = () => {
    if (!directory) throw new Error("Test output requires an active test attempt with a real file");
    return directory;
  };
  return {
    file,
    titlePath,
    project,
    get outputDir() {
      return requireDirectory();
    },
    outputPath(...segments) {
      return containedPath(project.outputDir, requireDirectory(), segments);
    },
  };
}
