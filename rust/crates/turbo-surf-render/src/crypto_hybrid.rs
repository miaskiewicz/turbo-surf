//! Native X-Wing keys and operations, with private cppgc material and permissions.
//! Composition follows the CFRG X-Wing construction used by Chrome 155.
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use deno_core::{cppgc, op2, v8, Extension, GarbageCollected};
use deno_error::JsErrorBox;
use fips203::{
    ml_kem_768,
    traits::{Decaps, Encaps, KeyGen, SerDes},
};
use serde_json::{json, Value};
use sha3::digest::{ExtendableOutput, Update, XofReader};
use std::ffi::CStr;
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

const NAME: &str = "MLKEM768-X25519";
const PUBLIC_LEN: usize = 1216;
const CIPHERTEXT_LEN: usize = 1120;
const USAGES: [(&str, u8); 4] = [
    ("encapsulateKey", 1),
    ("encapsulateBits", 2),
    ("decapsulateKey", 4),
    ("decapsulateBits", 8),
];
const PUBLIC_USAGES: u8 = 3;
const PRIVATE_USAGES: u8 = 12;

fn dom(name: &'static str, message: impl Into<String>) -> JsErrorBox {
    JsErrorBox::new(name, message.into())
}
fn access(message: impl Into<String>) -> JsErrorBox {
    dom("DOMExceptionInvalidAccessError", message)
}
fn data(message: impl Into<String>) -> JsErrorBox {
    dom("DOMExceptionDataError", message)
}
fn operation(message: impl Into<String>) -> JsErrorBox {
    dom("DOMExceptionOperationError", message)
}
fn unsupported() -> JsErrorBox {
    dom(
        "DOMExceptionNotSupportedError",
        "Unsupported export key format for algorithm",
    )
}
fn usage_mask(usages: &[String], allowed: u8) -> Result<u8, JsErrorBox> {
    let mut mask = 0;
    for usage in usages {
        let bit = USAGES
            .iter()
            .find(|(name, _)| *name == usage)
            .map(|(_, bit)| *bit)
            .filter(|bit| bit & allowed != 0)
            .ok_or_else(|| {
                dom(
                    "DOMExceptionSyntaxError",
                    "Unsupported key usage for X-Wing",
                )
            })?;
        mask |= bit;
    }
    Ok(mask)
}
fn usage_names(mask: u8) -> Vec<&'static str> {
    USAGES
        .iter()
        .filter(|(_, bit)| mask & bit != 0)
        .map(|(name, _)| *name)
        .collect()
}

struct PrivateMaterial {
    seed: Zeroizing<[u8; 32]>,
    mlkem: ml_kem_768::DecapsKey,
    x25519: Zeroizing<[u8; 32]>,
}
struct XWing {
    public: [u8; PUBLIC_LEN],
    private: Option<Box<PrivateMaterial>>,
}
impl XWing {
    fn from_seed(seed: [u8; 32]) -> Self {
        let seed = Zeroizing::new(seed);
        let mut expansion = Zeroizing::new([0u8; 96]);
        let mut shake = sha3::Shake256::default();
        Update::update(&mut shake, seed.as_ref());
        shake.finalize_xof().read(expansion.as_mut());
        let (public, private) = ml_kem_768::KG::keygen_from_seed(
            expansion[..32].try_into().unwrap(),
            expansion[32..64].try_into().unwrap(),
        );
        let x25519 = Zeroizing::new(expansion[64..].try_into().unwrap());
        let xpublic = x25519_dalek::x25519(*x25519, x25519_dalek::X25519_BASEPOINT_BYTES);
        let mut bytes = [0u8; PUBLIC_LEN];
        bytes[..1184].copy_from_slice(&public.into_bytes());
        bytes[1184..].copy_from_slice(&xpublic);
        Self {
            public: bytes,
            private: Some(Box::new(PrivateMaterial {
                seed,
                mlkem: private,
                x25519,
            })),
        }
    }
    fn from_public(bytes: &[u8]) -> Result<Self, JsErrorBox> {
        let bytes: [u8; PUBLIC_LEN] = bytes
            .try_into()
            .map_err(|_| data("Invalid X-Wing public key length"))?;
        ml_kem_768::EncapsKey::try_from_bytes(bytes[..1184].try_into().unwrap()).map_err(data)?;
        Ok(Self {
            public: bytes,
            private: None,
        })
    }
    fn public_only(&self) -> Self {
        Self {
            public: self.public,
            private: None,
        }
    }
    fn encapsulate(&self) -> Result<(Vec<u8>, Vec<u8>), JsErrorBox> {
        let mut entropy = Zeroizing::new([0u8; 64]);
        getrandom::fill(entropy.as_mut()).map_err(|_| operation("Entropy unavailable"))?;
        self.encapsulate_with_entropy(&entropy)
    }
    fn encapsulate_with_entropy(
        &self,
        entropy: &[u8; 64],
    ) -> Result<(Vec<u8>, Vec<u8>), JsErrorBox> {
        let public = ml_kem_768::EncapsKey::try_from_bytes(self.public[..1184].try_into().unwrap())
            .map_err(data)?;
        let (mlsecret, ciphertext) = public.encaps_from_seed(entropy[..32].try_into().unwrap());
        let mlsecret = Zeroizing::new(mlsecret.into_bytes());
        let ephemeral = x25519_dalek::x25519(
            entropy[32..].try_into().unwrap(),
            x25519_dalek::X25519_BASEPOINT_BYTES,
        );
        let xpublic: [u8; 32] = self.public[1184..].try_into().unwrap();
        let xsecret = Zeroizing::new(x25519_dalek::x25519(
            entropy[32..].try_into().unwrap(),
            xpublic,
        ));
        if xsecret.as_ref().ct_eq(&[0u8; 32]).unwrap_u8() == 1 {
            return Err(operation("Invalid X25519 public key"));
        }
        let secret = combine(&mlsecret, &xsecret, &ephemeral, &xpublic);
        let mut ciphertext = ciphertext.into_bytes().to_vec();
        ciphertext.extend_from_slice(&ephemeral);
        Ok((ciphertext, secret.to_vec()))
    }
    fn decapsulate(&self, ciphertext: &[u8]) -> Result<Vec<u8>, JsErrorBox> {
        let private = self
            .private
            .as_ref()
            .ok_or_else(|| access("The key is not of the expected type"))?;
        if ciphertext.len() != CIPHERTEXT_LEN {
            return Err(operation("Invalid X-Wing ciphertext length"));
        }
        let cipher = ml_kem_768::CipherText::try_from_bytes(ciphertext[..1088].try_into().unwrap())
            .map_err(operation)?;
        let mlsecret = Zeroizing::new(
            private
                .mlkem
                .try_decaps(&cipher)
                .map_err(operation)?
                .into_bytes(),
        );
        let ephemeral: [u8; 32] = ciphertext[1088..].try_into().unwrap();
        let xsecret = Zeroizing::new(x25519_dalek::x25519(*private.x25519, ephemeral));
        if xsecret.as_ref().ct_eq(&[0u8; 32]).unwrap_u8() == 1 {
            return Err(operation("Invalid X25519 ciphertext"));
        }
        Ok(combine(
            &mlsecret,
            &xsecret,
            &ephemeral,
            &self.public[1184..].try_into().unwrap(),
        )
        .to_vec())
    }
}
fn combine(
    mlsecret: &[u8; 32],
    xsecret: &[u8; 32],
    ephemeral: &[u8; 32],
    public: &[u8; 32],
) -> Zeroizing<[u8; 32]> {
    let mut hash = sha3::Sha3_256::default();
    for bytes in [
        mlsecret.as_slice(),
        xsecret.as_slice(),
        ephemeral.as_slice(),
        public.as_slice(),
        &[0x5c, 0x2e, 0x2f, 0x2f, 0x5e, 0x5c],
    ] {
        Update::update(&mut hash, bytes);
    }
    Zeroizing::new(sha3::Digest::finalize(hash).into())
}

pub(crate) struct NativeHybridKey {
    material: XWing,
    extractable: bool,
    usages: u8,
}

pub(crate) fn is_hybrid_key(scope: &mut v8::PinScope, value: v8::Local<v8::Value>) -> bool {
    cppgc::try_unwrap_cppgc_object::<NativeHybridKey>(scope, value).is_some()
}

pub(crate) struct HybridKeySnapshot {
    public: Box<[u8; PUBLIC_LEN]>,
    seed: Option<Zeroizing<[u8; 32]>>,
    extractable: bool,
    usages: u8,
}
impl HybridKeySnapshot {
    pub(crate) fn capture(scope: &mut v8::PinScope, value: v8::Local<v8::Value>) -> Option<Self> {
        let key = cppgc::try_unwrap_cppgc_object::<NativeHybridKey>(scope, value)?;
        Some(Self {
            public: Box::new(key.material.public),
            seed: key
                .material
                .private
                .as_ref()
                .map(|private| Zeroizing::new(*private.seed)),
            extractable: key.extractable,
            usages: key.usages,
        })
    }
    pub(crate) fn size_bytes(&self) -> usize {
        PUBLIC_LEN + if self.seed.is_some() { 32 } else { 0 }
    }
    pub(crate) fn restore<'s>(
        &self,
        scope: &mut v8::PinScope<'s, '_>,
    ) -> v8::Local<'s, v8::Object> {
        let material = match &self.seed {
            Some(seed) => XWing::from_seed(**seed),
            None => XWing {
                public: *self.public,
                private: None,
            },
        };
        cppgc::make_cppgc_object(
            scope,
            NativeHybridKey {
                material,
                extractable: self.extractable,
                usages: self.usages,
            },
        )
    }
}
// The cppgc object owns Rust crypto material only; it has no V8 references.
unsafe impl GarbageCollected for NativeHybridKey {
    fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}
    fn get_name(&self) -> &'static CStr {
        c"NativeHybridKey"
    }
}
impl NativeHybridKey {
    fn require(&self, private: bool, usage: u8) -> Result<(), JsErrorBox> {
        if self.material.private.is_some() != private || self.usages & usage == 0 {
            return Err(access("Key type or usage does not permit this operation"));
        }
        Ok(())
    }
    fn export_bytes_inner(&self, format: &str) -> Result<Vec<u8>, JsErrorBox> {
        if !self.extractable {
            return Err(access("key is not extractable"));
        }
        match format {
            "raw-public" => {
                if self.material.private.is_some() {
                    return Err(access("The key is not of the expected type"));
                }
                Ok(self.material.public.to_vec())
            }
            "raw-seed" => self
                .material
                .private
                .as_ref()
                .map(|private| private.seed.to_vec())
                .ok_or_else(|| access("The key is not of the expected type")),
            _ => Err(unsupported()),
        }
    }
    fn import_bytes_inner(
        format: &str,
        bytes: &[u8],
        extractable: bool,
        usages: &[String],
    ) -> Result<Self, JsErrorBox> {
        let (material, allowed) = match format {
            "raw-public" => (XWing::from_public(bytes)?, PUBLIC_USAGES),
            "raw-seed" => {
                let seed: [u8; 32] = bytes
                    .try_into()
                    .map_err(|_| data("Invalid X-Wing seed length"))?;
                (XWing::from_seed(seed), PRIVATE_USAGES)
            }
            _ => return Err(unsupported()),
        };
        let usages = usage_mask(usages, allowed)?;
        if material.private.is_some() && usages == 0 {
            return Err(dom(
                "DOMExceptionSyntaxError",
                "Usages cannot be empty for a private key",
            ));
        }
        Ok(Self {
            material,
            extractable,
            usages,
        })
    }
    fn jwk_value(&self) -> Result<serde_json::Value, JsErrorBox> {
        if !self.extractable {
            return Err(access("key is not extractable"));
        }
        let mut value = json!({"alg":NAME,"ext":true,"key_ops":usage_names(self.usages),"kty":"AKP","pub":URL_SAFE_NO_PAD.encode(self.material.public)});
        if let Some(private) = &self.material.private {
            value["priv"] = Value::String(URL_SAFE_NO_PAD.encode(private.seed.as_ref()));
        }
        Ok(value)
    }

    fn import_jwk_inner(
        jwk: serde_json::Value,
        extractable: bool,
        usages: Vec<String>,
    ) -> Result<NativeHybridKey, JsErrorBox> {
        if jwk["kty"] != "AKP" || jwk.get("alg").is_some_and(|value| value != NAME) {
            return Err(data("JWK algorithm or type does not match X-Wing"));
        }
        if jwk.get("ext") == Some(&Value::Bool(false)) && extractable {
            return Err(data("JWK is not extractable"));
        }
        if !usages.is_empty() && jwk.get("use").is_some_and(|value| value != "enc") {
            return Err(data("Invalid JWK use"));
        }
        let public = jwk["pub"]
            .as_str()
            .ok_or_else(|| data("JWK public key is required"))?;
        let public = URL_SAFE_NO_PAD
            .decode(public)
            .map_err(|_| data("Invalid JWK public key"))?;
        let private = jwk.get("priv").and_then(Value::as_str);
        let key = if let Some(private) = private {
            let seed = Zeroizing::new(
                URL_SAFE_NO_PAD
                    .decode(private)
                    .map_err(|_| data("Invalid JWK seed"))?,
            );
            NativeHybridKey::import_bytes_inner("raw-seed", &seed, extractable, &usages)?
        } else {
            NativeHybridKey::import_bytes_inner("raw-public", &public, extractable, &usages)?
        };
        if public.as_slice() != key.material.public {
            return Err(data("JWK public key does not match private seed"));
        }
        if let Some(value) = jwk.get("key_ops") {
            let declared = value
                .as_array()
                .ok_or_else(|| data("Invalid JWK key_ops"))?;
            let mut seen = Vec::new();
            for usage in declared {
                let usage = usage.as_str().ok_or_else(|| data("Invalid JWK key_ops"))?;
                if seen.contains(&usage) {
                    return Err(data("Duplicate JWK key_ops"));
                }
                seen.push(usage);
            }
            if usages.iter().any(|usage| !seen.contains(&usage.as_str())) {
                return Err(data("JWK key_ops do not permit requested usages"));
            }
        }
        Ok(key)
    }
}
fn key_object<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    key: NativeHybridKey,
) -> v8::Local<'s, v8::Object> {
    cppgc::make_cppgc_object(scope, key)
}
fn object_field<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    object: v8::Local<'s, v8::Object>,
    name: &str,
    value: v8::Local<'s, v8::Value>,
) {
    let name = v8::String::new(scope, name).unwrap();
    object.create_data_property(scope, name.into(), value);
}
fn array_buffer<'s>(scope: &mut v8::PinScope<'s, '_>, bytes: Vec<u8>) -> v8::Local<'s, v8::Value> {
    let backing = v8::ArrayBuffer::new_backing_store_from_boxed_slice(bytes.into_boxed_slice());
    v8::ArrayBuffer::with_backing_store(scope, &backing.make_shared()).into()
}

#[op2]
impl NativeHybridKey {
    #[constructor]
    #[cppgc]
    fn constructor() -> Result<NativeHybridKey, JsErrorBox> {
        Err(JsErrorBox::type_error("Illegal constructor"))
    }
    #[getter]
    #[rename("type")]
    #[string]
    fn key_type(&self) -> &'static str {
        if self.material.private.is_some() {
            "private"
        } else {
            "public"
        }
    }
    #[getter]
    fn extractable(&self) -> bool {
        self.extractable
    }
    #[getter]
    #[serde]
    fn algorithm(&self) -> serde_json::Value {
        json!({"name":NAME})
    }
    #[getter]
    #[serde]
    fn usages(&self) -> Vec<&'static str> {
        usage_names(self.usages)
    }
    #[fast]
    #[rename("isKey")]
    #[static_method]
    fn is_key(scope: &mut v8::PinScope, key: v8::Local<v8::Value>) -> bool {
        cppgc::try_unwrap_cppgc_object::<NativeHybridKey>(scope, key).is_some()
    }
    #[fast]
    #[rename("supports")]
    #[required(1)]
    #[static_method]
    fn supports(#[string] operation: &str) -> bool {
        matches!(
            operation,
            "generateKey"
                | "importKey"
                | "getPublicKey"
                | "encapsulateKey"
                | "encapsulateBits"
                | "decapsulateKey"
                | "decapsulateBits"
        )
    }
    #[required(2)]
    #[static_method]
    fn generate<'s>(
        scope: &mut v8::PinScope<'s, '_>,
        extractable: bool,
        #[serde] usages: Vec<String>,
    ) -> Result<v8::Local<'s, v8::Object>, JsErrorBox> {
        let mask = usage_mask(&usages, PUBLIC_USAGES | PRIVATE_USAGES)?;
        if mask & PRIVATE_USAGES == 0 {
            return Err(dom(
                "DOMExceptionSyntaxError",
                "Usages cannot be empty for a private key",
            ));
        }
        let mut seed = Zeroizing::new([0u8; 32]);
        getrandom::fill(seed.as_mut()).map_err(|_| operation("Entropy unavailable"))?;
        let material = XWing::from_seed(*seed);
        let public = key_object(
            scope,
            NativeHybridKey {
                material: material.public_only(),
                extractable: true,
                usages: mask & PUBLIC_USAGES,
            },
        );
        let private = key_object(
            scope,
            NativeHybridKey {
                material,
                extractable,
                usages: mask & PRIVATE_USAGES,
            },
        );
        let pair = v8::Object::new(scope);
        object_field(scope, pair, "publicKey", public.into());
        object_field(scope, pair, "privateKey", private.into());
        Ok(pair)
    }
    #[cppgc]
    #[rename("importBytes")]
    #[required(4)]
    #[static_method]
    fn import_bytes(
        #[string] format: &str,
        #[buffer(copy)] bytes: Vec<u8>,
        extractable: bool,
        #[serde] usages: Vec<String>,
    ) -> Result<NativeHybridKey, JsErrorBox> {
        NativeHybridKey::import_bytes_inner(format, &bytes, extractable, &usages)
    }
    #[cppgc]
    #[rename("getPublicKey")]
    #[required(2)]
    #[static_method]
    fn public_key(
        #[cppgc] key: &NativeHybridKey,
        #[serde] usages: Vec<String>,
    ) -> Result<NativeHybridKey, JsErrorBox> {
        if key.material.private.is_none() {
            return Err(access("The key is not of the expected type"));
        }
        Ok(NativeHybridKey {
            material: key.material.public_only(),
            extractable: true,
            usages: usage_mask(&usages, PUBLIC_USAGES)?,
        })
    }
    #[arraybuffer]
    #[rename("exportBytes")]
    #[required(2)]
    #[static_method]
    fn export_bytes(
        #[string] format: &str,
        #[cppgc] key: &NativeHybridKey,
    ) -> Result<Vec<u8>, JsErrorBox> {
        key.export_bytes_inner(format)
    }
    #[rename("exportJwk")]
    #[required(1)]
    #[serde]
    #[static_method]
    fn export_jwk(#[cppgc] key: &NativeHybridKey) -> Result<serde_json::Value, JsErrorBox> {
        key.jwk_value()
    }
    #[arraybuffer]
    #[rename("exportSerializedJwk")]
    #[required(1)]
    #[static_method]
    fn export_serialized_jwk(#[cppgc] key: &NativeHybridKey) -> Result<Vec<u8>, JsErrorBox> {
        serde_json::to_vec(&key.jwk_value()?).map_err(|_| operation("JWK serialization failed"))
    }
    #[cppgc]
    #[rename("importJwk")]
    #[required(3)]
    #[static_method]
    fn import_jwk(
        #[serde] jwk: serde_json::Value,
        extractable: bool,
        #[serde] usages: Vec<String>,
    ) -> Result<NativeHybridKey, JsErrorBox> {
        NativeHybridKey::import_jwk_inner(jwk, extractable, usages)
    }
    #[cppgc]
    #[rename("importSerializedJwk")]
    #[required(3)]
    #[static_method]
    fn import_serialized_jwk(
        #[buffer(copy)] bytes: Vec<u8>,
        extractable: bool,
        #[serde] usages: Vec<String>,
    ) -> Result<NativeHybridKey, JsErrorBox> {
        let bytes = Zeroizing::new(bytes);
        let jwk = serde_json::from_slice(&bytes).map_err(|_| data("Wrapped JWK is invalid"))?;
        NativeHybridKey::import_jwk_inner(jwk, extractable, usages)
    }
    #[rename("encapsulateBits")]
    #[required(2)]
    #[static_method]
    fn encapsulate<'s>(
        scope: &mut v8::PinScope<'s, '_>,
        #[cppgc] key: &NativeHybridKey,
        key_operation: bool,
    ) -> Result<v8::Local<'s, v8::Object>, JsErrorBox> {
        key.require(false, if key_operation { 1 } else { 2 })?;
        let (ciphertext, secret) = key.material.encapsulate()?;
        let result = v8::Object::new(scope);
        let ciphertext = array_buffer(scope, ciphertext);
        object_field(scope, result, "ciphertext", ciphertext);
        let secret = array_buffer(scope, secret);
        object_field(scope, result, "sharedKey", secret);
        Ok(result)
    }
    #[arraybuffer]
    #[rename("decapsulateBits")]
    #[required(3)]
    #[static_method]
    fn decapsulate(
        #[cppgc] key: &NativeHybridKey,
        #[buffer(copy)] ciphertext: Vec<u8>,
        key_operation: bool,
    ) -> Result<Vec<u8>, JsErrorBox> {
        key.require(true, if key_operation { 4 } else { 8 })?;
        key.material.decapsulate(&ciphertext)
    }
}

deno_core::extension!(turbo_surf_hybrid_crypto, objects = [NativeHybridKey]);
pub(crate) fn extension() -> Extension {
    turbo_surf_hybrid_crypto::init()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::Digest;
    #[test]
    fn known_seed_matches_real_chrome155_public_key() {
        let key = XWing::from_seed(std::array::from_fn(|index| index as u8));
        assert_eq!(
            format!("{:x}", sha2::Sha256::digest(key.public)),
            "c9a3565ffde4f72b51661be391ee13e46378d7f06dd5c8bf5af9d2cfb5b8336b"
        );
        let entropy = std::array::from_fn(|index| index as u8);
        let (ciphertext, secret) = key.encapsulate_with_entropy(&entropy).unwrap();
        assert_eq!(ciphertext.len(), 1120);
        assert_eq!(secret.len(), 32);
        assert_eq!(key.decapsulate(&ciphertext).unwrap(), secret);
        assert_eq!(
            format!("{:x}", sha2::Sha256::digest(secret)),
            "2346ac22902583e940637aae3e669a9f754a63fe7cf2c2b39c1c52c21f94de28"
        );
    }
    #[test]
    fn hybrid_material_rejects_malformed_and_low_order_inputs() {
        assert!(XWing::from_public(&[0; 1215]).is_err());
        let mut public = XWing::from_seed([1; 32]).public;
        public[1184..].fill(0);
        let key = XWing::from_public(&public).unwrap();
        assert!(key.encapsulate_with_entropy(&[2; 64]).is_err());
        let private = XWing::from_seed([1; 32]);
        let (mut ciphertext, _) = private.encapsulate_with_entropy(&[2; 64]).unwrap();
        ciphertext[1088..].fill(0);
        assert!(private.decapsulate(&ciphertext).is_err());
        assert!(private.decapsulate(&[0; 1119]).is_err());
    }
    #[test]
    fn native_permissions_and_export_formats_are_enforced() {
        let private = NativeHybridKey::import_bytes_inner(
            "raw-seed",
            &[7; 32],
            false,
            &["decapsulateBits".into()],
        )
        .unwrap();
        assert!(private.export_bytes_inner("raw-seed").is_err());
        assert!(private.require(true, 4).is_err());
        assert!(private.require(true, 8).is_ok());
        let public = NativeHybridKey {
            material: private.material.public_only(),
            extractable: true,
            usages: 2,
        };
        assert_eq!(public.export_bytes_inner("raw-public").unwrap().len(), 1216);
        assert!(public.export_bytes_inner("raw-seed").is_err());
        assert!(public.export_bytes_inner("raw").is_err());
    }
    #[test]
    fn chrome_ciphertext_decapsulates_with_the_native_test_seed() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../harness/detection/fixtures/webcrypto-xwing-chrome155.json"
        ))
        .unwrap();
        let encoded = fixture["ciphertextHex"].as_str().unwrap();
        let ciphertext: Vec<u8> = (0..encoded.len())
            .step_by(2)
            .map(|index| u8::from_str_radix(&encoded[index..index + 2], 16).unwrap())
            .collect();
        let key = XWing::from_seed(std::array::from_fn(|index| index as u8));
        let secret = key.decapsulate(&ciphertext).unwrap();
        assert_eq!(
            format!("{:x}", sha2::Sha256::digest(secret)),
            fixture["sharedSha256"].as_str().unwrap()
        );
    }
}
