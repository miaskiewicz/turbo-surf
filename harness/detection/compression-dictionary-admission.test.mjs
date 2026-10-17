import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";

test("manual dictionary fetch guard uses captured intrinsics without changing retained headers", () => {
  const source = readFileSync(
    new URL(
      "../../rust/crates/turbo-surf-render/src/compression_dictionary_fetch.js",
      import.meta.url,
    ),
    "utf8",
  );
  const context = vm.createContext({});
  vm.runInContext(
    `const __baseApply=Reflect.apply,__exceptionTypeError=TypeError;${source}\nglobalThis.admit=__rejectManualDictionaryHeader;globalThis.NativeTypeError=TypeError;`,
    context,
  );
  vm.runInContext(
    "Object.keys=()=>{throw Error('guest keys')};String.prototype.toLowerCase=()=>{throw Error('guest lowercase')};",
    context,
  );
  for (const name of ["available-dictionary", "Available-Dictionary", "AVAILABLE-DICTIONARY"]) {
    const headers = { [name]: ":AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=:" };
    assert.throws(
      () => context.admit(headers),
      (error) => error instanceof context.NativeTypeError && error.message === "Failed to fetch",
    );
    assert.equal(headers[name], ":AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=:");
  }
  const identifier = { "Dictionary-ID": '"forged"' };
  assert.doesNotThrow(() => context.admit(identifier));
  assert.deepEqual(identifier, { "Dictionary-ID": '"forged"' });
  assert.doesNotThrow(() => context.admit(Object.create({ "Available-Dictionary": "inherited" })));
});
