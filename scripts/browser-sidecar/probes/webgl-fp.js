// WebGL fingerprint differential: run the standard-ish fingerprint draw (shaders + a triangle,
// readPixels) + the identity strings, hash the pixels. Run in real Chrome (ANGLE) and the
// turbo-surf gpu-metal bridge; compare — is our render REAL (non-blank, varied), Apple-CONSISTENT
// (renderer strings match the pixels' provenance), and how close is the pixel hash to Chrome's?
//
//   real Chrome : node scripts/browser-sidecar/run-probe.mjs scripts/browser-sidecar/probes/webgl-fp.js
//   turbo-surf  : cargo run -p turbo-surf-mcp --features gpu-metal --example fp_snapshot -- --render scripts/browser-sidecar/probes/webgl-fp.js
//
// NB: the WebGL GPU bridge only runs on the ASYNC render path — use fp_snapshot's `--render`
// flag (the sync eval runtime does not drive the bridge). Compare `hash.centerRGBA` + `pixelHash`.
(function () {
  const S = (fn) => {
    try {
      return fn();
    } catch (e) {
      return "<throw:" + (e && e.name) + ">";
    }
  };
  const c = document.createElement("canvas");
  c.width = 256;
  c.height = 128;
  const gl = c.getContext("webgl") || c.getContext("experimental-webgl");
  if (!gl) return JSON.stringify({ error: "no-webgl" });

  const dbg = gl.getExtension("WEBGL_debug_renderer_info");
  const identity = {
    vendor: S(() => gl.getParameter(gl.VENDOR)),
    renderer: S(() => gl.getParameter(gl.RENDERER)),
    unmaskedVendor: dbg ? S(() => gl.getParameter(dbg.UNMASKED_VENDOR_WEBGL)) : "<no-dbg>",
    unmaskedRenderer: dbg ? S(() => gl.getParameter(dbg.UNMASKED_RENDERER_WEBGL)) : "<no-dbg>",
    version: S(() => gl.getParameter(gl.VERSION)),
    aliasedLineWidthRange: S(() => Array.from(gl.getParameter(gl.ALIASED_LINE_WIDTH_RANGE) || [])),
    maxViewportDims: S(() => Array.from(gl.getParameter(gl.MAX_VIEWPORT_DIMS) || [])),
  };

  // Draw a gradient triangle (a common fingerprint scene: colours interpolate → many distinct
  // pixel values, so a real rasterizer produces a rich hash and a blank stub is obvious).
  const hash = S(() => {
    const vs = gl.createShader(gl.VERTEX_SHADER);
    gl.shaderSource(
      vs,
      "attribute vec2 p; varying vec2 v; void main(){ v = p; gl_Position = vec4(p, 0.0, 1.0); }",
    );
    gl.compileShader(vs);
    const fs = gl.createShader(gl.FRAGMENT_SHADER);
    gl.shaderSource(
      fs,
      "precision highp float; varying vec2 v; void main(){ gl_FragColor = vec4(v.x*0.5+0.5, v.y*0.5+0.5, 0.6, 1.0); }",
    );
    gl.compileShader(fs);
    const pr = gl.createProgram();
    gl.attachShader(pr, vs);
    gl.attachShader(pr, fs);
    gl.linkProgram(pr);
    gl.useProgram(pr);
    const buf = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, buf);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW);
    const loc = gl.getAttribLocation(pr, "p");
    gl.enableVertexAttribArray(loc);
    gl.vertexAttribPointer(loc, 2, gl.FLOAT, false, 0, 0);
    gl.viewport(0, 0, c.width, c.height);
    gl.clearColor(0, 0, 0, 1);
    gl.clear(gl.COLOR_BUFFER_BIT);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
    const px = new Uint8Array(c.width * c.height * 4);
    gl.readPixels(0, 0, c.width, c.height, gl.RGBA, gl.UNSIGNED_BYTE, px);
    // FNV-ish hash + stats.
    let h = 2166136261 >>> 0,
      nonZero = 0,
      distinct = {};
    for (let i = 0; i < px.length; i++) {
      h = (h ^ px[i]) >>> 0;
      h = (h * 16777619) >>> 0;
      if (px[i] !== 0) nonZero++;
    }
    // sample a few center pixels to eyeball the gradient
    const mid = ((c.height >> 1) * c.width + (c.width >> 1)) * 4;
    return {
      pixelHash: h >>> 0,
      nonZeroBytes: nonZero,
      totalBytes: px.length,
      centerRGBA: [px[mid], px[mid + 1], px[mid + 2], px[mid + 3]],
    };
  });

  return JSON.stringify({
    identity,
    hash,
    extCount: S(() => (gl.getSupportedExtensions() || []).length),
  });
})();
