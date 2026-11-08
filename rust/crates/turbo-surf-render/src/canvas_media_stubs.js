// Presence stubs for the three canvas/media members Chrome 156 exposes but turbo-surf
// has no media pipeline for (turbo-surf#106, split out of #28). These satisfy
// feature-detection code paths (`typeof x === 'function'`, `'captureStream' in canvas`)
// and return the typed shapes Chrome returns, with no media data. Real behaviour
// (OffscreenCanvas draws reaching the placeholder canvas, a MediaStream carrying a
// CanvasCaptureMediaStreamTrack, a PictureInPictureWindow promise) stays a follow-up
// in turbo-dom-browser-env's binding — the binding owns the canvas/media pipeline.
(() => {
  "use strict";
  const g = globalThis;
  const define = Object.defineProperty;
  const Canvas = g.HTMLCanvasElement && g.HTMLCanvasElement.prototype;
  const Video = g.HTMLVideoElement && g.HTMLVideoElement.prototype;
  const Offscreen = g.OffscreenCanvas;
  const Media = g.MediaStream;
  const Exception = g.DOMException;
  const PromiseType = g.Promise;
  // oxlint-disable-next-line consistent-function-scoping
  const branded = (ctor) => {
    if (typeof ctor !== "function") return {};
    try {
      return Reflect.construct(ctor, []);
    } catch {
      return ctor.prototype ? Object.create(ctor.prototype) : {};
    }
  };
  // oxlint-disable-next-line consistent-function-scoping
  const brandedWith = (ctor, args) => {
    if (typeof ctor !== "function") return {};
    try {
      return Reflect.construct(ctor, args);
    } catch {
      return ctor.prototype ? Object.create(ctor.prototype) : {};
    }
  };
  const configurableMethod = (owner, name, fn, length) => {
    if (!owner || name in owner) return;
    define(fn, "name", { __proto__: null, value: name });
    define(fn, "length", { __proto__: null, value: length });
    define(owner, name, {
      __proto__: null,
      value: fn,
      writable: true,
      configurable: true,
      enumerable: true,
    });
  };
  configurableMethod(
    Canvas,
    "captureStream",
    function captureStream(_frameRequestRate) {
      // Chrome returns a MediaStream with a CanvasCaptureMediaStreamTrack. We return a
      // tracks-empty MediaStream — same type, no frames (no paint pipeline).
      return branded(Media);
    },
    0,
  );
  configurableMethod(
    Canvas,
    "transferControlToOffscreen",
    function transferControlToOffscreen() {
      // Chrome detaches the backing bitmap and returns a bound OffscreenCanvas; we hand
      // back a sized OffscreenCanvas shell with no bitmap link.
      const width = Number(this && this.width) || 0;
      const height = Number(this && this.height) || 0;
      return brandedWith(Offscreen, [width, height]);
    },
    0,
  );
  configurableMethod(
    Video,
    "requestPictureInPicture",
    function requestPictureInPicture() {
      // No media data (html_media.js leaves video at HAVE_NOTHING), so Chrome's spec
      // rejects with InvalidStateError "Metadata for the video element are not loaded
      // yet." We match that typed reject so promise-chain detection succeeds.
      const message = "Metadata for the video element are not loaded yet.";
      const error =
        typeof Exception === "function"
          ? new Exception(message, "InvalidStateError")
          : new TypeError(message);
      return PromiseType.reject(error);
    },
    0,
  );
})();
