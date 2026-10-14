fn main() {
    build_audio();
    build_effects();
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

fn build_audio() {
    let root = "native/audio";
    let files = [
        "compressor.cc",
        "timeline_math.cc",
        "third_party/blink/renderer/platform/audio/dynamics_compressor.cc",
        "third_party/fdlibm/ieee754.cc",
    ];
    let mut build = cc::Build::new();
    build
        .cpp(true)
        .std("c++20")
        .include(root)
        .define("NDEBUG", None);
    // Blink deliberately disables contraction: FMA changes the rendered PCM.
    build.flag_if_supported("-ffp-contract=off");
    for file in files {
        build.file(format!("{root}/{file}"));
    }
    println!("cargo:rerun-if-changed={root}");
    build.compile("turbo_surf_audio");
}

fn build_effects() {
    let root = "native/audio";
    let mut build = cc::Build::new();
    build
        .cpp(true)
        .std("c++20")
        .include(format!("{root}/effects/include"))
        .include(root)
        .define("NDEBUG", None)
        .flag_if_supported("-ffp-contract=off");
    for file in [
        "biquad.cc",
        "delay.cc",
        "delay_neon.cc",
        "stereo_panner.cc",
        "adapter.cc",
    ] {
        build.file(format!("{root}/effects/{file}"));
    }
    build.flag_if_supported("-Wno-unused-parameter");
    build.compile("turbo_surf_audio_effects");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-lib=framework=Accelerate");
    }
}
