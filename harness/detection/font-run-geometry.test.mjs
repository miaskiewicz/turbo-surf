import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import test from "node:test";

const fixture = JSON.parse(
  readFileSync(new URL("./fixtures/font-run-geometry-chrome155.json", import.meta.url)),
);
const sha = (path) =>
  createHash("sha256")
    .update(readFileSync(new URL(path, import.meta.url)))
    .digest("hex");

test("same-font oracle retains normal Chrome policy, executed sources and actual font provenance", () => {
  assert.match(fixture.browser, /^Chrome\/155\./);
  assert.equal(fixture.v8, "15.5.35.23");
  assert.equal(fixture.mode, "headed-direct-cdp");
  assert.equal(fixture.probeSha256, sha("./font-run-geometry-probe.js"));
  assert.equal(fixture.collectorSha256, sha("./font-run-geometry-chrome-control.mjs"));
  assert.equal(fixture.socketHelperSha256, sha("./websocket-open.mjs"));
  assert.deepEqual(fixture.errors, []);
  assert.ok(fixture.flags.includes("--user-data-dir=<fresh-profile>"));
  assert.ok(fixture.flags.every((flag) => !/headless|font-render|font-subpixel/.test(flag)));
  assert.equal(fixture.producer.repository, "miaskiewicz/turbo-html2pdf");
  assert.equal(fixture.producer.commit, "2eac7f64142d8e6bde37f7ae46006c991b6f2347");
  assert.deepEqual(
    fixture.fonts.map(({ family, sha256 }) => [family, sha256]),
    [
      ["IBM Plex Mono", "6a3412f058c7d8dfd9170c41e85ade48e5156ecb89356110ca57a0a27734af46"],
      ["Fira Code", "5992ab9640e2df491b2f609467b1de60e8bc39b2c28db184342a0592d98f6117"],
    ],
  );
  assert.ok(
    fixture.fonts.every((font) => font.publicPinnedBytesIdentical && !font.path.startsWith("/")),
  );
  assert.equal(fixture.result.length, 9);
  for (const row of fixture.result) {
    if (row.geometry.family !== "monospace") {
      assert.equal(row.fontSetup.faceStatus, "loaded");
      assert.ok(
        row.platformFonts.some(
          (font) =>
            font.isCustomFont && font.familyName === row.geometry.family && font.glyphCount === 3,
        ),
      );
    }
    assert.equal(Object.keys(row.geometry.decoration.checks).length, 7);
    assert.ok(Object.values(row.geometry.decoration.checks).every(Boolean));
    assert.equal(row.geometry.runs.length, 10);
    assert.deepEqual(
      row.geometry.splits.parts.map((part) => part.text),
      ["a", "bc"],
    );
    assert.equal(row.geometry.rotated.ranges.length, 5);
  }
});

test("same-font partial Range evidence preserves independent interval rounding", () => {
  for (const row of fixture.result.filter((row) => row.geometry.family === "IBM Plex Mono")) {
    const pair = row.geometry.runs.find((run) => run.text === "ab");
    const first = pair.ranges.find((range) => range.start === 0 && range.end === 1);
    const second = pair.ranges.find((range) => range.start === 1 && range.end === 2);
    assert.equal(first.rect.width, 9.6015625);
    assert.equal(second.rect.width, 9.609375);
    assert.notEqual(first.rect.width, second.rect.width);
    for (const text of ["abcde", "a b c"]) {
      const run = row.geometry.runs.find((item) => item.text === text);
      assert.equal(run.element.width, 48);
      assert.notEqual(run.element.width, first.rect.width * 5);
    }
    const full = row.geometry.runs.find((run) => run.text === "abc");
    assert.equal(full.element.width, 28.8046875);
    assert.equal(
      full.ranges.find((range) => range.start === 0 && range.end === 3).rect.width,
      full.element.width,
    );
  }
  for (const row of fixture.result.filter((row) => row.geometry.family === "Fira Code")) {
    const total = row.geometry.splits.parts.reduce((sum, part) => sum + part.element.width, 0);
    assert.equal(total, 29.546875);
    assert.equal(row.geometry.splits.whole.element.width, 29.5390625);
    assert.notEqual(total, row.geometry.splits.whole.element.width);
  }
});

test("Range transform sibling retains exact source provenance and distinct real boundaries", () => {
  const control = JSON.parse(
    readFileSync(new URL("./fixtures/range-transform-stages-chrome155.json", import.meta.url)),
  );
  assert.equal(control.mode, "headed-direct-cdp");
  assert.equal(control.probeKind, "range-transforms");
  assert.equal(control.probeSha256, sha("./range-transform-stages-probe.js"));
  assert.equal(control.collectorSha256, sha("./font-run-geometry-chrome-control.mjs"));
  assert.equal(control.socketHelperSha256, sha("./websocket-open.mjs"));
  assert.deepEqual(control.errors, []);
  assert.deepEqual(control.fonts, fixture.fonts);
  assert.deepEqual(
    control.result.map((row) => row.geometry.family),
    ["IBM Plex Mono", "Fira Code"],
  );
  for (const row of control.result) {
    assert.equal(row.geometry.cases.length, 11);
    assert.ok(
      row.platformFonts.some(
        (font) =>
          font.isCustomFont && font.familyName === row.geometry.family && font.glyphCount === 3,
      ),
    );
    for (const entry of row.geometry.cases) assert.equal(entry.transformed.ranges.length, 6);
  }
  const cases = control.result[1].geometry.cases;
  const full = (name) =>
    cases
      .find((entry) => entry.name === name)
      .transformed.ranges.find((range) => range.start === 0 && range.end === 3).rect;
  assert.equal(full("margin-flow").x, 2.305981159210205);
  assert.equal(full("padding-flow").x, 2.305980920791626);
  assert.deepEqual(full("padding-flow"), full("absolute"));
  assert.notEqual(full("margin-flow").width, full("padding-flow").width);
});
