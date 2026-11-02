//! X-Wing (MLKEM768-X25519) JWK import/export and usage validation. Expected results
//! (error names; Chrome's messages are its own) are Chrome 156 (156.0.8078.4, headless)
//! running the same script on a localhost page.
use serde_json::Value;

const SCRIPT: &str = r#"(async()=>{const log=[];const t=async(n,f)=>{try{const r=await f();log.push([n,r])}catch(e){log.push([n,e.name])}};
const s=crypto.subtle;
const pair=await s.generateKey({name:'MLKEM768-X25519'},true,['encapsulateBits','decapsulateBits','encapsulateKey','decapsulateKey']).catch(e=>e);
log.push(['gen',String(pair&&pair.privateKey&&pair.privateKey.type)]);
if(pair.privateKey){
 const jwk=await s.exportKey('jwk',pair.privateKey);const pjwk=await s.exportKey('jwk',pair.publicKey);
 log.push(['jwk keys',Object.keys(jwk).sort().join(),jwk.kty,jwk.alg,jwk.key_ops.join()]);
 const imp=(n,j,ext,u)=>t(n,async()=>{const k=await s.importKey('jwk',j,{name:'MLKEM768-X25519'},ext,u);return k.type+':'+k.usages.join()+':'+k.extractable});
 await imp('ok priv',jwk,true,['decapsulateBits']);
 await imp('ok pub',pjwk,true,['encapsulateBits']);
 await imp('bad kty',{...jwk,kty:'OKP'},true,['decapsulateBits']);
 await imp('bad alg',{...jwk,alg:'X'},true,['decapsulateBits']);
 await imp('ext false',{...jwk,ext:false},true,['decapsulateBits']);
 await imp('use sig',{...jwk,use:'sig'},true,['decapsulateBits']);
 await imp('no pub',{...jwk,pub:undefined},true,['decapsulateBits']);
 await imp('bad pub b64',{...jwk,pub:'@@@'},true,['decapsulateBits']);
 await imp('bad priv b64',{...jwk,priv:'@@@'},true,['decapsulateBits']);
 await imp('mismatch',{...jwk,pub:pjwk.pub.slice(0,-4)+'AAAA'},true,['decapsulateBits']);
 await imp('dup ops',{...jwk,key_ops:['decapsulateBits','decapsulateBits']},true,['decapsulateBits']);
 await imp('ops deny',{...jwk,key_ops:['encapsulateBits']},true,['decapsulateBits']);
 await imp('ops not array',{...jwk,key_ops:'x'},true,['decapsulateBits']);
 await imp('priv empty usages',jwk,true,[]);
 await t('export nonext',async()=>{const k=await s.importKey('jwk',jwk,{name:'MLKEM768-X25519'},false,['decapsulateBits']);return await s.exportKey('jwk',k)});
 await t('raw-public of private',async()=>(await s.exportKey('raw-public',pair.privateKey)).byteLength);
 await t('raw-seed of public',async()=>(await s.exportKey('raw-seed',pair.publicKey)).byteLength);
 await t('getPublicKey of public',async()=>(await s.getPublicKey(pair.publicKey,['encapsulateBits'])).type);
 await t('gen pub only usages',async()=>(await s.generateKey({name:'MLKEM768-X25519'},true,['encapsulateBits'])).privateKey.type);
 await t('raw-seed bad usage',async()=>(await s.importKey('raw-seed',new Uint8Array(32),{name:'MLKEM768-X25519'},true,['encapsulateBits'])).type);
 await t('raw-seed empty usage',async()=>(await s.importKey('raw-seed',new Uint8Array(32),{name:'MLKEM768-X25519'},true,[])).type);
}
globalThis.__RESULT=JSON.stringify(log);})()"#;

const CHROME_156: &str = r#"[["gen","private"],["jwk keys","alg,ext,key_ops,kty,priv,pub","AKP","MLKEM768-X25519","decapsulateKey,decapsulateBits"],["ok priv","private:decapsulateBits:true"],["ok pub","public:encapsulateBits:true"],["bad kty","DataError"],["bad alg","DataError"],["ext false","DataError"],["use sig","DataError"],["no pub","DataError"],["bad pub b64","DataError"],["bad priv b64","DataError"],["mismatch","DataError"],["dup ops","DataError"],["ops deny","DataError"],["ops not array","TypeError"],["priv empty usages","SyntaxError"],["export nonext","InvalidAccessError"],["raw-public of private","InvalidAccessError"],["raw-seed of public","InvalidAccessError"],["getPublicKey of public","InvalidAccessError"],["gen pub only usages","SyntaxError"],["raw-seed bad usage","SyntaxError"],["raw-seed empty usage","SyntaxError"]]"#;

#[tokio::test(flavor = "current_thread")]
async fn hybrid_jwk_import_and_export_errors_match_chrome() {
    let result = turbo_surf_render::eval_async(
        "<!doctype html><body></body>",
        "https://hybrid.test/",
        SCRIPT,
    )
    .await
    .unwrap();
    let log: Value = serde_json::from_str(&result).unwrap();
    let chrome: Value = serde_json::from_str(CHROME_156).unwrap();
    assert_eq!(log, chrome);
}
