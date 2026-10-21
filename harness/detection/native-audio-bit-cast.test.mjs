import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { superviseTestChild } from "./child-supervision.mjs";

const headers = fileURLToPath(
  new URL("../../rust/crates/turbo-surf-render/native/audio/", import.meta.url),
);
const fixture = fileURLToPath(new URL("./fixtures/native-audio-bit-cast.cc", import.meta.url));

function expectFinished(result) {
  assert.equal(result.timedOut, false, result.stderr);
  assert.equal(result.aborted, false, result.stderr);
  assert.equal(result.error, null);
  assert.equal(result.signal, null, result.stderr);
}

test(
  "native audio bit casts retain every floating-point bit with native and legacy libraries",
  { timeout: 45_000 },
  async (t) => {
    const directory = mkdtempSync(join(tmpdir(), "turbo-audio-bit-cast-"));
    const children = [];
    let passed = false;
    t.after(async () => {
      await Promise.allSettled(children);
      if (passed) rmSync(directory, { recursive: true, force: true });
      else t.diagnostic(`Retained compiler evidence: ${directory}`);
    });
    let sequence = 0;
    function run(command, args) {
      const pending = superviseTestChild(t, command, args, {
        timeoutMs: 10_000,
        spawnOptions: { cwd: directory },
      }).then((result) => {
        writeFileSync(
          join(directory, `${sequence++}-process.json`),
          JSON.stringify({ command, args, result }, null, 2),
        );
        return result;
      });
      children.push(pending);
      return pending;
    }
    const candidates = [
      ...new Set(
        [
          process.env.CXX,
          "c++",
          "clang++",
          "g++",
          ...(process.platform === "win32" ? ["cl.exe"] : []),
        ].filter(Boolean),
      ),
    ];
    let compiler;
    let msvc;
    for (const candidate of candidates) {
      const isMsvc = /^cl(?:\.exe)?$/i.test(basename(candidate));
      const result = await run(candidate, [isMsvc ? "/Bv" : "--version"]);
      if (result.error?.includes("ENOENT")) continue;
      expectFinished(result);
      compiler = candidate;
      msvc = isMsvc;
      break;
    }
    if (!compiler) {
      passed = true;
      t.skip("No C++ compiler is installed (CXX/c++/clang++/g++/cl.exe)");
      return;
    }
    const emptyBit = join(directory, "legacy-headers");
    mkdirSync(emptyBit);
    writeFileSync(
      join(emptyBit, "bit"),
      "// A standard library with no bit_cast declaration or feature macro.\n",
    );
    const compile = (source, output, legacy) =>
      run(
        compiler,
        msvc
          ? [
              "/nologo",
              "/std:c++20",
              "/EHsc",
              "/O2",
              ...(legacy ? [`/I${emptyBit}`, "/DTURBO_SURF_TEST_EXPECT_LEGACY=1"] : []),
              `/I${headers}`,
              source,
              `/Fe${output}`,
              `/Fo${output}.obj`,
            ]
          : [
              "-std=c++20",
              "-O2",
              "-Wall",
              "-Wextra",
              ...(legacy ? ["-I", emptyBit, "-DTURBO_SURF_TEST_EXPECT_LEGACY=1"] : []),
              "-I",
              headers,
              source,
              "-o",
              output,
            ],
      );
    for (const legacy of [false, true]) {
      const executable = join(
        directory,
        `roundtrip-${legacy}${process.platform === "win32" ? ".exe" : ""}`,
      );
      const built = await compile(fixture, executable, legacy);
      expectFinished(built);
      assert.equal(built.code, 0, built.stderr);
      const ran = await run(executable, []);
      expectFinished(ran);
      assert.equal(ran.code, 0, ran.stderr);
      if (legacy) assert.equal(ran.stdout.trim(), "fallback");
      else assert.match(ran.stdout.trim(), /^(std|fallback)$/);
      t.diagnostic(`${legacy ? "empty-bit library" : "native library"}: ${ran.stdout.trim()}`);
    }
    const invalid = {
      size: [
        "std::uint64_t source=0; (void)base::bit_cast<std::uint32_t>(source);",
        /bit_cast requires equal sizes/,
      ],
      source: [
        "struct X {std::uint64_t n; ~X(){}}; X source{}; (void)base::bit_cast<std::uint64_t>(source);",
        /bit_cast source must be trivially copyable/,
      ],
      destination: [
        "struct X {std::uint64_t n; ~X(){}}; std::uint64_t source=0; (void)base::bit_cast<X>(source);",
        /bit_cast destination must be trivially copyable/,
      ],
    };
    for (const [name, [body, message]] of Object.entries(invalid)) {
      const source = join(directory, `${name}.cc`);
      writeFileSync(
        source,
        `#include <cstdint>\n#include "base/bit_cast.h"\nint main(){${body}}\n`,
      );
      const result = await compile(source, join(directory, `${name}.out`), true);
      expectFinished(result);
      assert.notEqual(result.code, 0, `${name} unexpectedly compiled`);
      assert.match(result.stdout + result.stderr, message);
    }
    assert.equal(readFileSync(join(emptyBit, "bit"), "utf8").startsWith("//"), true);
    passed = true;
  },
);
