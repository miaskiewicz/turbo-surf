# Hardened native WebCrypto provider

turbo-surf's `turbo-surf-deno-crypto` distribution is the complete published
MIT-licensed `deno_crypto` **0.273.0** source with a narrowly scoped browser
embedding patch. No patched source tree is committed: only the patch, the pins
and the generator. The source archive is
[`deno_crypto-0.273.0.crate`](https://static.crates.io/crates/deno_crypto/deno_crypto-0.273.0.crate).
Its SHA-256 is
`d9931d22328961b9a9c8d3b666a27312c1f3ddcc7ea78d8610ddfbc934ac61a0`.
The archive records upstream commit
[`0c071246a412575e07423263404a5d13e7ed6aa2`](https://github.com/denoland/deno/tree/0c071246a412575e07423263404a5d13e7ed6aa2/ext/crypto).
The Deno MIT license is retained (the patch adds it as `LICENSE.md`).

[`patches/deno_crypto.patch`](patches/deno_crypto.patch) is the complete
difference from the published archive:

- Native result objects and arrays use `CreateDataProperty`, preventing inherited
  page setters from receiving private key material or replacing native metadata.
- Raw JavaScript key snapshots are created only when upstream Node/clone symbol
  support is explicitly registered. Turbo-surf never registers that support.
- Rust-only helpers prove genuine cppgc crypto brands and clone a native key
  handle, preserving type, extractability, algorithm, and usages. They do not
  export material to JavaScript or add public key factories or symbols.
- Queued key snapshots copy native material and typed algorithm metadata into
  Rust-owned records, including public exponents, usages and permissions. No
  sender wrapper, expando, prototype, JavaScript metadata dictionary or array is
  rooted by a queue. Delivery builds fresh receiver-realm native key state; queue
  admission includes the owned material bytes. The host-only `NativeKeySnapshot`
  API has private fields and does not install a page-accessible exporter.
- Crypto/SubtleCrypto and CryptoKey algorithm/usages/handle edges use actual
  cppgc traced references. Their native owners preserve identity while live and
  release whole iframe realms when collected, instead of creating strong Rust
  Global roots.
- Deprecated AWS-LC ML-DSA aliases use the corresponding stable APIs; this keeps
  the reviewed provider warning-free with the locked upstream version.

The hostile `Object.prototype.keyData` regression previously observed 16 raw
bytes when importing a non-extractable AES key. The fixed regression observes
zero setter calls and zero material bytes. This is a provider construction
boundary, independent of Google detection.

## Reproduce and verify

[`prepare.py`](prepare.py) is the generator: it unpacks the checksum-pinned
archive (from Cargo's cache or a private `--archive-dir`, never modifying the
cache), applies the patch with `patch -p1`, and requires the result to equal the
`patchedFiles` tree pinned in [`source-pins.json`](source-pins.json) before it
renames the package and adds the embedded-source table:

```sh
python3 rust/release-deps/deno/prepare.py \
  --archive-dir .native-builds/release-deno-upstream-archives --download \
  --output .native-builds/release-deno-candidates
```

To change the patch, edit an unpacked archive, regenerate
`patches/deno_crypto.patch` with `diff -ruN` against the pristine archive, update
`patchedFiles` (SHA-256 per file) and re-run the command above plus
`python3 -m unittest discover -s rust/release-deps/deno -p 'test_*.py'`.

The render crate consumes the named package `turbo-surf-deno-crypto` (0.273.1,
the reviewed distribution of upstream 0.273.0) through its unchanged
`deno_crypto` Rust alias. The matching Deno core, WebIDL and web extension
packages share the named Deno family on turbo-dom's `turbo-dom-v8` fork,
preserving native types and op registration. Actual packaged and registry
consumers must retain the private-key setter and native snapshot controls;
matching package names or a successful upload alone is not that proof. Do not
silently fall back to unpatched upstream `deno_crypto`.
