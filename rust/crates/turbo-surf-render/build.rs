fn main() {
    println!("cargo:rustc-check-cfg=cfg(turbo_surf_native_proxy)");
    println!("cargo:rerun-if-env-changed=TURBO_SURF_ENGINE");
    let version = match std::env::var("TURBO_SURF_ENGINE").as_deref() {
        Ok("154") => Some("15.4.80.20-rusty"),
        Ok("155") => Some("15.5.35.20-rusty"),
        Ok(value) => panic!(
            "unsupported TURBO_SURF_ENGINE {value:?}; use the revision-pinned engine build helper"
        ),
        Err(_) => None,
    };
    if let Some(version) = version {
        println!("cargo:rustc-cfg=turbo_surf_native_proxy");
        println!("cargo:rustc-env=TURBO_SURF_EXPECTED_NATIVE_V8={version}");
    }
}
