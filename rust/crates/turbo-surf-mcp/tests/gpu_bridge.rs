//! Optional hardware controls: compiled only in the selected Metal feature profile.
#![cfg(feature = "gpu-metal")]
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::{json, Value};
use turbo_dom_raster as raster;

const PROBE: &str = include_str!("../../../../harness/detection/gpu-bridge-validation-probe.js");

fn metadata() -> Value {
    let value: Value =
        serde_json::from_str(&raster::webgl_metadata().expect("real Metal adapter required"))
            .unwrap();
    assert_eq!(value["backend"], "Metal");
    assert!(!value["name"].as_str().unwrap().is_empty());
    eprintln!(
        "gpu_hardware_receipt={}",
        json!({"v8":turbo_surf_render::v8_version(),
        "gpuMetal":cfg!(feature="gpu-metal"),"coretext":cfg!(feature="coretext"),"adapter":value})
    );
    value
}
fn solid(pixels: &[u8], color: [u8; 4]) {
    assert_eq!(pixels.len(), 8 * 8 * 4);
    assert!(
        pixels.chunks_exact(4).all(|pixel| pixel == color),
        "full-frame shader color {color:?}"
    );
}
fn batch(color: &str) -> String {
    let vertices: Vec<u8> = [-1_f32, -1., 3., -1., -1., 3.]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    json!([
        {"m":"clearColor","a":[0.,0.,0.,1.]},{"m":"clear","a":[16384]},
        {"m":"createShader","id":1,"a":[35633]},
        {"m":"shaderSource","a":[1,"attribute vec2 aPos;void main(){gl_Position=vec4(aPos,0.0,1.0);}"]},
        {"m":"compileShader","a":[1]},{"m":"createShader","id":2,"a":[35632]},
        {"m":"shaderSource","a":[2,format!("precision mediump float;void main(){{gl_FragColor=vec4({color});}}") ]},
        {"m":"compileShader","a":[2]},{"m":"createProgram","id":3},
        {"m":"attachShader","a":[3,1]},{"m":"attachShader","a":[3,2]},
        {"m":"attribLocation","a":[3,0,"aPos"]},{"m":"linkProgram","a":[3]},
        {"m":"useProgram","a":[3]},{"m":"createBuffer","id":4},
        {"m":"bindBuffer","a":[34962,4]},
        {"m":"bufferData","a":[34962,{"b64":STANDARD.encode(vertices),"t":"f32"},35044]},
        {"m":"enableVertexAttribArray","a":[0]},{"m":"vertexAttribPointer","a":[0,2,5126,false,0,0]},
        {"m":"drawArrays","a":[4,0,3]}
    ]).to_string()
}
#[test]
fn metal_adapter_and_direct_shader_pixels_are_real() {
    metadata();
    let red = raster::webgl_readback(8, 8, &batch("1.0,0.0,0.0,1.0")).expect("GPU red readback");
    let blue = raster::webgl_readback(8, 8, &batch("0.0,0.0,1.0,1.0")).expect("GPU blue readback");
    solid(&red, [255, 0, 0, 255]);
    solid(&blue, [0, 0, 255, 255]);
    assert_ne!(red, blue);
}
#[tokio::test(flavor = "current_thread")]
async fn installed_mcp_bridge_returns_exact_shader_pixels_and_adapter_identity() {
    turbo_surf_mcp::initialize_render_hooks();
    let adapter = metadata();
    let mut page = turbo_surf_render::PageSession::open(
        "<!doctype html><body>",
        "https://gpu-feature.test/",
        "",
        "",
        15000,
    )
    .await
    .unwrap();
    let result = page
        .eval_json_expression(PROBE.trim().trim_end_matches(';'))
        .await
        .unwrap();
    page.close();
    let result: Value = serde_json::from_str(&result).unwrap();
    for (name, color) in [("red", [255, 0, 0, 255]), ("blue", [0, 0, 255, 255])] {
        assert_eq!(result[name]["renderer"], adapter["renderer"]);
        assert_eq!(result[name]["vendor"], adapter["vendor"]);
        let pixels: Vec<u8> = serde_json::from_value(result[name]["pixels"].clone()).unwrap();
        solid(&pixels, color);
    }
    assert_ne!(result["red"]["pixels"], result["blue"]["pixels"]);
}
