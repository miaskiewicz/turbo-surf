use deno_core::{
    JsRuntime, ModuleLoadOptions, ModuleLoadReferrer, ModuleLoadResponse, ModuleLoader,
    ModuleResolveResponse, ModuleSource, ModuleSourceCode, ModuleSpecifier, ModuleType,
    ResolutionKind, RuntimeOptions, SourceCodeCacheInfo, resolve_import, v8,
};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    future::Future,
    hash::{Hash, Hasher},
    pin::Pin,
    rc::Rc,
    sync::Arc,
};

struct Loader {
    units: HashMap<String, Arc<[u16]>>,
    compilations: Cell<usize>,
    cache: RefCell<HashMap<String, (u64, Vec<u8>)>>,
    cache_hits: Cell<usize>,
}
impl ModuleLoader for Loader {
    fn resolve(
        &self,
        specifier: &str,
        referrer: &str,
        _kind: ResolutionKind,
    ) -> ModuleResolveResponse {
        resolve_import(specifier, referrer)
            .map_err(|e| deno_error::JsErrorBox::generic(e.to_string()))
    }
    fn load(
        &self,
        url: &ModuleSpecifier,
        _referrer: Option<&ModuleLoadReferrer>,
        _options: ModuleLoadOptions,
    ) -> ModuleLoadResponse {
        let code = if self.units.contains_key(url.as_str()) {
            "throw new Error('scalar compatibility source was compiled')"
        } else {
            "import {show, where, fail} from './child.js';globalThis.first=show();const again=await import('./child.js');globalThis.same=again.show===show;globalThis.source=show.toString();globalThis.where=where;try{fail()}catch(e){globalThis.stack=e.stack};"
        };
        let cache = self.units.get(url.as_str()).map(|units| {
            let mut hash = std::collections::hash_map::DefaultHasher::new();
            units.hash(&mut hash);
            let hash = hash.finish();
            let data = self
                .cache
                .borrow()
                .get(url.as_str())
                .filter(|(stored, _)| *stored == hash)
                .map(|(_, bytes)| bytes.clone().into());
            if data.is_some() {
                self.cache_hits.set(self.cache_hits.get() + 1);
            }
            SourceCodeCacheInfo { hash, data }
        });
        ModuleLoadResponse::Sync(Ok(ModuleSource::new(
            ModuleType::JavaScript,
            ModuleSourceCode::String(code.to_owned().into()),
            url,
            cache,
        )))
    }
    fn code_cache_ready(
        &self,
        specifier: ModuleSpecifier,
        hash: u64,
        code: &[u8],
    ) -> Pin<Box<dyn Future<Output = ()>>> {
        self.cache
            .borrow_mut()
            .insert(specifier.to_string(), (hash, code.to_vec()));
        Box::pin(async {})
    }
    fn module_source_url(&self, specifier: &str) -> String {
        if self.units.contains_key(specifier) {
            "https://native.test/document.html".into()
        } else {
            specifier.into()
        }
    }
    fn module_source_code_units(&self, specifier: &str) -> Option<Arc<[u16]>> {
        let units = self.units.get(specifier)?.clone();
        self.compilations.set(self.compilations.get() + 1);
        Some(units)
    }
}
fn evaluate(runtime: &mut JsRuntime, source: &str) -> String {
    let value = runtime
        .execute_script("native-control.js", source.to_owned())
        .unwrap();
    deno_core::scope!(scope, runtime);
    let value = v8::Local::new(scope, value);
    value.to_rust_string_lossy(scope)
}
#[tokio::test]
async fn native_module_compilation_preserves_units_identity_locations_and_isolate_maps() {
    let code = "export function show(){return 'X'}\nexport function fail(){throw new Error('boom')}\nexport const where=import.meta.url;globalThis.childLoads=(globalThis.childLoads||0)+1;";
    let mut units: Vec<u16> = code.encode_utf16().collect();
    let marker = code.find('X').unwrap();
    units[marker] = 0xd800;
    units.insert(marker + 1, 0xdc01);
    units.insert(marker + 2, 0xd802);
    let loader = Rc::new(Loader {
        units: HashMap::from([("https://native.test/child.js".into(), units.into())]),
        compilations: Cell::new(0),
        cache: RefCell::new(HashMap::new()),
        cache_hits: Cell::new(0),
    });
    for _ in 0..2 {
        let mut runtime = JsRuntime::new(RuntimeOptions {
            module_loader: Some(loader.clone()),
            ..Default::default()
        });
        let url = ModuleSpecifier::parse("https://native.test/main.js").unwrap();
        let id = runtime.load_main_es_module(&url).await.unwrap();
        let evaluation = runtime.mod_evaluate(id);
        runtime.run_event_loop(Default::default()).await.unwrap();
        evaluation.await.unwrap();
        assert_eq!(
            evaluate(
                &mut runtime,
                "JSON.stringify(Array.from(first,(_,i)=>first.charCodeAt(i)))"
            ),
            "[55296,56321]"
        );
        assert_eq!(
            evaluate(
                &mut runtime,
                "JSON.stringify(Array.from({length:first.length},(_,i)=>first.charCodeAt(i)))"
            ),
            "[55296,56321,55298]"
        );
        assert_eq!(
            evaluate(
                &mut runtime,
                "String(same&&childLoads===1&&source===\"function show(){return '\\uD800\\uDC01\\uD802'}\")"
            ),
            "true"
        );
        assert_eq!(
            evaluate(&mut runtime, "where"),
            "https://native.test/document.html"
        );
        assert_eq!(
            evaluate(
                &mut runtime,
                "String(stack.includes('https://native.test/document.html:2:30'))"
            ),
            "true"
        );
    }
    assert_eq!(loader.compilations.get(), 2);
    assert_eq!(loader.cache_hits.get(), 1);
    assert!(!loader.cache.borrow().values().next().unwrap().1.is_empty());
    println!(
        "V8 {} original units and two independent module maps passed",
        v8::V8::get_version()
    );
}
