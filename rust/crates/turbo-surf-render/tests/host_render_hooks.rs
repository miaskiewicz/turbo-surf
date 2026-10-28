//! Host-injected render hooks (installed by the crates that own fonts, raster and GPU:
//! mcp/napi). Process-global, so this binary installs them once before any page runs:
//! canvas `measureText` widths come from the text measurer, `toDataURL` PNG bytes and
//! `getImageData` pixels from the rasterizers, and the WebGL unmasked identity from the
//! adapter metadata.
use std::sync::Once;
use turbo_surf_render::PageSession;

const PNG: &[u8] = b"\x89PNG\r\n\x1a\nhost-raster";

fn install() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        turbo_surf_render::set_measure_fn(Box::new(|text, family, size| {
            // A recognizable measurer: 10px per character, +1 per pixel of size for "Wide".
            let extra = if family == "Wide" { size } else { 0.0 };
            (text.chars().count() as f64 * 10.0 + extra, size)
        }));
        turbo_surf_render::set_raster_fn(Box::new(|_, _, _| Some(PNG.to_vec())));
        turbo_surf_render::set_raster_rgba_fn(Box::new(|width, height, ops| {
            // Pixel (x, y) = [x, y, ops.len() % 256, 255].
            let mut pixels = Vec::with_capacity((width * height * 4) as usize);
            for y in 0..height {
                for x in 0..width {
                    pixels.extend([x as u8, y as u8, (ops.len() % 256) as u8, 255]);
                }
            }
            Some(pixels)
        }));
        turbo_surf_render::set_webgl_metadata_fn(Box::new(|| {
            Some(r#"{"vendor":"Host Vendor","renderer":"Host Renderer"}"#.into())
        }));
    });
}

#[tokio::test(flavor = "current_thread")]
async fn canvas_and_webgl_use_the_installed_host_hooks() {
    install();
    let mut page = PageSession::open("<body>", "https://hooks.test/", "", "", 5000)
        .await
        .unwrap();
    let value = page
        .eval_json_expression(
            r#"(()=>{
  const canvas=document.createElement('canvas');canvas.width=4;canvas.height=3;
  const ctx=canvas.getContext('2d');ctx.fillStyle='red';ctx.fillRect(0,0,4,3);
  ctx.font='20px Wide';const wide=ctx.measureText('abc').width;
  ctx.font='12px "Plain", serif';const plain=ctx.measureText('abcd').width;
  const pixel=Array.from(ctx.getImageData(2,1,1,1).data);
  const url=canvas.toDataURL();
  const gl=document.createElement('canvas').getContext('webgl');
  const info=gl&&gl.getExtension('WEBGL_debug_renderer_info');
  const identity=info?[gl.getParameter(info.UNMASKED_VENDOR_WEBGL),gl.getParameter(info.UNMASKED_RENDERER_WEBGL)]:null;
  return {wide,plain,pixel:pixel.slice(0,2).concat(pixel[3]),url,identity};
})()"#,
        )
        .await
        .unwrap();
    let value: serde_json::Value = serde_json::from_str(&value).unwrap();
    use base64::Engine;
    let png = base64::engine::general_purpose::STANDARD.encode(PNG);
    assert_eq!(
        value,
        serde_json::json!({
            "wide": 50,
            "plain": 40,
            "pixel": [2, 1, 255],
            "url": format!("data:image/png;base64,{png}"),
            "identity": ["Host Vendor", "Host Renderer"],
        })
    );
}
