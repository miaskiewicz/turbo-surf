# Hardened native WebCrypto provider

This directory preserves the complete published MIT-licensed
`deno_crypto` **0.273.0** source, with a narrowly scoped browser embedding patch.
The source archive is
[`deno_crypto-0.273.0.crate`](https://static.crates.io/crates/deno_crypto/deno_crypto-0.273.0.crate).
Its SHA-256 is
`d9931d22328961b9a9c8d3b666a27312c1f3ddcc7ea78d8610ddfbc934ac61a0`.
The archive records upstream commit
[`0c071246a412575e07423263404a5d13e7ed6aa2`](https://github.com/denoland/deno/tree/0c071246a412575e07423263404a5d13e7ed6aa2/ext/crypto).
The [Deno MIT license](deno_crypto/LICENSE.md) is retained.

`deno_crypto.patch` is the complete difference from the published archive:

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
- Deprecated AWS-LC ML-DSA aliases use the corresponding stable APIs; this keeps
  the local path dependency warning-free with the locked provider version.

The hostile `Object.prototype.keyData` regression previously observed 16 raw
bytes when importing a non-extractable AES key. The fixed regression observes
zero setter calls and zero material bytes. This is a provider construction
boundary, independent of Google detection.

Reproduce and verify without modifying the package cache:

```sh
python3 rust/vendor/update-deno-crypto.py --check
```

The helper finds the exact archive in Cargo's cache. Pass `--archive PATH` if it
is elsewhere; `--write` rebuilds the vendored source from that hash-verified
archive and the committed patch. No registry source files or SDK source files
are modified.

The workspace applies this source with a Cargo patch. A downstream registry
consumer does not inherit workspace patches, so a Surf crate release must first
provide a distributable hardened provider dependency or an upstream release
containing these fixes. Packaging checks must not silently fall back to the
unpatched registry provider.
