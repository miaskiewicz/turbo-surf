(() => {
  const draw = (color) => {
    const canvas = document.createElement("canvas");
    canvas.width = canvas.height = 8;
    const gl = canvas.getContext("webgl");
    if (!gl) throw new Error("WebGL context unavailable");
    const debug = gl.getExtension("WEBGL_debug_renderer_info");
    if (!debug) throw new Error("WebGL adapter identity unavailable");
    const shader = (kind, source) => {
      const value = gl.createShader(kind);
      gl.shaderSource(value, source);
      gl.compileShader(value);
      return value;
    };
    const program = gl.createProgram();
    gl.attachShader(
      program,
      shader(gl.VERTEX_SHADER, "attribute vec2 aPos;void main(){gl_Position=vec4(aPos,0.0,1.0);}"),
    );
    gl.attachShader(
      program,
      shader(
        gl.FRAGMENT_SHADER,
        `precision mediump float;void main(){gl_FragColor=vec4(${color});}`,
      ),
    );
    gl.linkProgram(program);
    gl.useProgram(program);
    const buffer = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW);
    const location = gl.getAttribLocation(program, "aPos");
    gl.enableVertexAttribArray(location);
    gl.vertexAttribPointer(location, 2, gl.FLOAT, false, 0, 0);
    gl.viewport(0, 0, 8, 8);
    gl.clearColor(0, 0, 0, 1);
    gl.clear(gl.COLOR_BUFFER_BIT);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
    const pixels = new Uint8Array(8 * 8 * 4);
    gl.readPixels(0, 0, 8, 8, gl.RGBA, gl.UNSIGNED_BYTE, pixels);
    return {
      vendor: gl.getParameter(debug.UNMASKED_VENDOR_WEBGL),
      renderer: gl.getParameter(debug.UNMASKED_RENDERER_WEBGL),
      pixels: Array.from(pixels),
    };
  };
  return { red: draw("1.0,0.0,0.0,1.0"), blue: draw("0.0,0.0,1.0,1.0") };
})();
