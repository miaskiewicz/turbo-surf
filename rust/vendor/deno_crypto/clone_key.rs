// Copyright 2018-2026 the Deno authors. MIT license.
// Browser embedding additions by the turbo-surf contributors.

//! Native-only key cloning. No extraction permission or JS material snapshot.
use deno_core::cppgc::{make_cppgc_object, try_unwrap_cppgc_object};
use deno_core::v8;
use deno_error::JsErrorBox;

use crate::CryptoError;
use crate::crypto_key::{CryptoKey, CryptoKeyType};
use crate::make_key::{
  AlgorithmDict, build_algorithm_object, build_handle_object,
  build_usages_array,
};
use crate::shared::RawKeyData;

/// Tests the native cppgc brand without invoking JavaScript properties.
pub fn is_crypto_key(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> bool {
  try_unwrap_cppgc_object::<CryptoKey>(scope, value).is_some()
}

/// Native Crypto/SubtleCrypto objects are not structured-cloneable.
pub fn is_crypto_object(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> bool {
  is_crypto_key(scope, value)
    || try_unwrap_cppgc_object::<crate::crypto::Crypto>(scope, value).is_some()
    || try_unwrap_cppgc_object::<crate::subtle_crypto::SubtleCrypto>(
      scope, value,
    )
    .is_some()
}

/// Queued clones own only Rust data, never the sender wrapper or its JS graph.
pub struct NativeKeySnapshot {
  material: RawKeyData,
  key_type: CryptoKeyType,
  extractable: bool,
  usages: Vec<String>,
  algorithm: AlgorithmDict,
}

fn own<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  object: v8::Local<'s, v8::Object>,
  name: &str,
) -> Option<v8::Local<'s, v8::Value>> {
  let name = v8::String::new(scope, name)?;
  if object.has_own_property(scope, name.into()) != Some(true) {
    return None;
  }
  object.get(scope, name.into())
}
fn string(
  scope: &mut v8::PinScope,
  value: v8::Local<v8::Value>,
) -> Option<String> {
  value.is_string().then(|| value.to_rust_string_lossy(scope))
}
fn algorithm_snapshot<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  object: v8::Local<'s, v8::Object>,
) -> Option<AlgorithmDict> {
  let name = own(scope, object, "name")?;
  let mut dict = AlgorithmDict::new(string(scope, name)?);
  for (name, field) in [
    ("length", &mut dict.length),
    ("modulusLength", &mut dict.modulus_length),
  ] {
    if let Some(value) = own(scope, object, name) {
      if !value.is_uint32() {
        return None;
      }
      *field = Some(value.uint32_value(scope)?);
    }
  }
  if let Some(value) = own(scope, object, "namedCurve") {
    dict.named_curve = Some(string(scope, value)?);
  }
  if let Some(value) = own(scope, object, "hash") {
    let hash = v8::Local::<v8::Object>::try_from(value).ok()?;
    let name = own(scope, hash, "name")?;
    dict.hash_name = Some(string(scope, name)?);
  }
  if let Some(value) = own(scope, object, "publicExponent") {
    let exponent = v8::Local::<v8::Uint8Array>::try_from(value).ok()?;
    let mut bytes = vec![0; exponent.byte_length()];
    if exponent.copy_contents(&mut bytes) != bytes.len() {
      return None;
    }
    dict.public_exponent = Some(bytes);
  }
  Some(dict)
}

impl NativeKeySnapshot {
  /// Uses actual cppgc slots, without public getters or export permission.
  pub fn capture<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    value: v8::Local<'s, v8::Value>,
  ) -> Option<Self> {
    let key = try_unwrap_cppgc_object::<CryptoKey>(scope, value)?;
    let material = key.key_handle(scope)?.data().clone();
    let object = key.algorithm_local(scope)?;
    let algorithm = algorithm_snapshot(scope, object)?;
    Some(Self {
      material,
      key_type: key.key_type(),
      extractable: key.extractable_(),
      usages: key.usages_as_vec(scope)?,
      algorithm,
    })
  }
  /// Bytes owned by a queued clone, including material and typed metadata.
  pub fn size_bytes(&self) -> usize {
    let material = match &self.material {
      RawKeyData::SeededPrivate { seed, private_key } => private_key
        .len()
        .saturating_add(seed.as_ref().map_or(0, |seed| seed.len())),
      _ => self.material.bytes().len(),
    };
    [
      self.algorithm.name.len(),
      self.algorithm.hash_name.as_ref().map_or(0, String::len),
      self.algorithm.named_curve.as_ref().map_or(0, String::len),
      self.algorithm.public_exponent.as_ref().map_or(0, Vec::len),
    ]
    .into_iter()
    .chain(self.usages.iter().map(String::len))
    .fold(material, usize::saturating_add)
  }
  /// Reconstructs fresh receiver-realm state and native material on delivery.
  pub fn restore<'s>(
    &self,
    scope: &mut v8::PinScope<'s, '_>,
  ) -> v8::Local<'s, v8::Object> {
    let names: Vec<&str> = self.usages.iter().map(String::as_str).collect();
    let usages = build_usages_array(scope, &names);
    let algorithm = build_algorithm_object(scope, &self.algorithm);
    let handle = build_handle_object(scope, self.material.clone());
    let key = CryptoKey::from_parts(
      scope,
      self.key_type,
      self.extractable,
      usages.into(),
      algorithm.into(),
      handle.into(),
    );
    make_cppgc_object(scope, key)
  }
}

/// Copies native slots and permissions; the clone shares no JS metadata graph.
pub fn clone_key<'s>(
  scope: &mut v8::PinScope<'s, '_>,
  value: v8::Local<'s, v8::Value>,
) -> Result<v8::Local<'s, v8::Object>, CryptoError> {
  let snapshot = NativeKeySnapshot::capture(scope, value).ok_or_else(|| {
    CryptoError::Other(JsErrorBox::type_error("Invalid native CryptoKey"))
  })?;
  Ok(snapshot.restore(scope))
}
