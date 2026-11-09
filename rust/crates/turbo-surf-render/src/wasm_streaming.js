// WebAssembly streaming-compile handler. deno_core installs an isolate-wide V8
// callback for `WebAssembly.compileStreaming` / `instantiateStreaming` and
// panics ("wasm streaming callback invoked before the JS handler was set") if
// nothing was registered via `Deno.core.setWasmStreamingCallback` by the time a
// page script triggers a streaming compile. See turbo-surf#247.
//
// Any real-world site that uses `WebAssembly.*Streaming` (fonts, image codecs,
// crypto, analytics) would otherwise crash the whole render process. The handler
// captures `Deno.core` / its op table into its closure before the bootstrap step
// deletes the `Deno` global from page scope.
((core) => {
  const ops = core.ops;
  core.setWasmStreamingCallback(async (source, rid) => {
    try {
      const response = await source;
      let bytes;
      if (response && typeof response.arrayBuffer === "function") {
        if (typeof response.url === "string" && response.url.length > 0) {
          try {
            ops.op_wasm_streaming_set_url(rid, response.url);
          } catch {
            // Nothing to attribute the module to — proceed without a URL.
          }
        }
        bytes = new Uint8Array(await response.arrayBuffer());
      } else if (response instanceof ArrayBuffer) {
        bytes = new Uint8Array(response);
      } else if (ArrayBuffer.isView(response)) {
        bytes = new Uint8Array(response.buffer, response.byteOffset, response.byteLength);
      } else {
        throw new TypeError("WebAssembly streaming source must be a Response or BufferSource");
      }
      ops.op_wasm_streaming_feed(rid, bytes);
      core.close(rid);
    } catch (e) {
      try {
        ops.op_abort_wasm_streaming(rid, e);
      } catch {
        // The streaming resource was already closed; the compile is done.
      }
    }
  });
})(globalThis.Deno.core);
