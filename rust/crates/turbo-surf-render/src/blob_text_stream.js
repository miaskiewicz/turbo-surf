// Blob#textStream: the blob's bytes as UTF-8 strings, decoded like a
// TextDecoderStream over Blob#stream(). Blink reads blob data through a data
// pipe, so each 64 KiB chunk arrives in a later task, never synchronously.
{
  const states = __blobStates,
    Readable = ReadableStream,
    PromiseType = Promise,
    TypeErrorType = TypeError,
    Decoder = TextDecoder,
    decode = TextDecoder.prototype.decode,
    schedule = setTimeout;
  const controllerPrototype = ReadableStreamDefaultController.prototype,
    enqueue = controllerPrototype.enqueue,
    close = controllerPrototype.close;
  const subarray = Uint8Array.prototype.subarray,
    byteLength = Object.getOwnPropertyDescriptor(
      Object.getPrototypeOf(Uint8Array.prototype),
      "length",
    ).get;
  const apply = Reflect.apply,
    define = Object.defineProperty,
    descriptor = Object.getOwnPropertyDescriptor;
  const CHUNK = 65536;
  const textStream = __G.makeNativeFunction(
    function textStream() {
      const state = states.get(this);
      if (state === undefined) throw new TypeErrorType("Illegal invocation");
      const bytes = state.bytes,
        length = apply(byteLength, bytes, []),
        decoder = new Decoder("utf-8", { __proto__: null });
      let offset = 0;
      return new Readable(
        {
          __proto__: null,
          pull(controller) {
            return new PromiseType((done) =>
              schedule(() => {
                while (offset < length) {
                  const end = offset + CHUNK < length ? offset + CHUNK : length;
                  const text = apply(decode, decoder, [
                    apply(subarray, bytes, [offset, end]),
                    { __proto__: null, stream: true },
                  ]);
                  offset = end;
                  if (text) {
                    apply(enqueue, controller, [text]);
                    done();
                    return;
                  }
                }
                const rest = apply(decode, decoder, []);
                if (rest) apply(enqueue, controller, [rest]);
                apply(close, controller, []);
                done();
              }, 0),
            );
          },
        },
        { __proto__: null, highWaterMark: 0 },
      );
    },
    "textStream",
    0,
  );
  const proto = Blob.prototype,
    constructor = descriptor(proto, "constructor"),
    tag = descriptor(proto, Symbol.toStringTag);
  define(proto, "textStream", {
    value: textStream,
    writable: true,
    enumerable: true,
    configurable: true,
  });
  // Keep Chrome's member order: operations, then constructor, then the tag.
  delete proto.constructor;
  define(proto, "constructor", constructor);
  delete proto[Symbol.toStringTag];
  define(proto, Symbol.toStringTag, tag);
}
