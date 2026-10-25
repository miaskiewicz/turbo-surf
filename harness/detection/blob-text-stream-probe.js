(async () => {
  // Blob#textStream: shape, brand check, UTF-8 decoding across chunks, flush,
  // independence from page-patched stream helpers, and delivery after microtasks.
  const out = {};
  const attempt = (callback) => {
    try {
      return ["ok", Object.prototype.toString.call(callback())];
    } catch (error) {
      return [error.constructor.name, error.message];
    }
  };
  const read = async (stream) => {
    const reader = stream.getReader(),
      chunks = [];
    for (;;) {
      const { done, value } = await reader.read();
      if (done) return chunks;
      chunks.push(
        typeof value === "string" ? value : ["not-a-string", Object.prototype.toString.call(value)],
      );
    }
  };
  const d = Object.getOwnPropertyDescriptor(Blob.prototype, "textStream");
  out.descriptor = [
    d.value.name,
    d.value.length,
    d.enumerable,
    d.configurable,
    d.writable,
    Object.hasOwn(d.value, "prototype"),
    Function.prototype.toString.call(d.value).endsWith("{ [native code] }"),
  ];
  out.keys = Reflect.ownKeys(Blob.prototype).map(String);
  out.brand = [
    attempt(() => Blob.prototype.textStream.call({})),
    attempt(() => Blob.prototype.textStream.call(undefined)),
    attempt(() => Blob.prototype.textStream.call(new Blob([]))),
    attempt(() => Blob.prototype.textStream.call(new File(["x"], "x.txt"))),
  ];
  const bytes = (...values) => new Blob([new Uint8Array(values)]);
  out.empty = await read(new Blob([]).textStream());
  out.ascii = await read(new Blob(["hello"]).textStream());
  out.parts = await read(new Blob(["ab", "cd", new Uint8Array([0x65])]).textStream());
  out.bom = await read(bytes(0xef, 0xbb, 0xbf, 0x41).textStream());
  out.onlyBom = await read(bytes(0xef, 0xbb, 0xbf).textStream());
  out.invalid = await read(bytes(0x41, 0xff, 0xc3).textStream());
  out.splitParts = await read(
    new Blob([new Uint8Array([0xe2]), new Uint8Array([0x82, 0xac])]).textStream(),
  );
  out.utf16Bom = await read(bytes(0xff, 0xfe, 0x41, 0x00).textStream());
  out.typeIgnored = await read(
    new Blob([new Uint8Array([0xe9])], { type: "text/plain;charset=latin1" }).textStream(),
  );
  out.slice = await read(new Blob(["hello world"]).slice(6).textStream());
  // A sequence split across a 64 KiB read boundary still decodes whole; how
  // many chunks Chrome's data pipe delivers is not fixed, only their content.
  const edge = new Uint8Array(65537).fill(0x61);
  edge[65535] = 0xc3;
  edge[65536] = 0xa9;
  const edgeText = (await read(new Blob([edge]).textStream())).join("");
  out.chunkEdge = [
    edgeText.length,
    edgeText.codePointAt(edgeText.length - 1),
    edgeText.includes("\uFFFD"),
  ];
  // Larger blobs arrive in implementation-sized chunks; only their content is fixed.
  const big = new Uint8Array(300000);
  for (let index = 0; index < big.length; index++) big[index] = 0x61 + (index % 26);
  const bigChunks = await read(new Blob([big]).textStream());
  out.big = [
    bigChunks.every((chunk) => typeof chunk === "string" && chunk.length > 0),
    bigChunks.join("") === new TextDecoder().decode(big),
  ];
  const stream = new Blob(["x"]).textStream();
  out.stream = [
    stream instanceof ReadableStream,
    Object.getPrototypeOf(stream) === ReadableStream.prototype,
    stream.locked,
  ];
  out.twice = [
    await read(new Blob(["again"]).textStream()),
    await (async () => {
      const blob = new Blob(["again"]);
      await read(blob.textStream());
      return read(blob.textStream());
    })(),
  ];
  const log = [];
  const reader = new Blob(["abc"]).textStream().getReader();
  reader.read().then((result) => log.push(["read", result.value]));
  Promise.resolve()
    .then(() => log.push("m1"))
    .then(() => log.push("m2"))
    .then(() => log.push("m3"));
  await new Promise((done) => setTimeout(done, 50));
  out.afterMicrotasks = log;
  const pipeThrough = ReadableStream.prototype.pipeThrough,
    TextDecoderStreamBefore = globalThis.TextDecoderStream,
    decode = TextDecoder.prototype.decode,
    enqueue = ReadableStreamDefaultController.prototype.enqueue;
  const touched = [];
  ReadableStream.prototype.pipeThrough = function () {
    touched.push("pipeThrough");
    return pipeThrough.apply(this, arguments);
  };
  globalThis.TextDecoderStream = function () {
    touched.push("TextDecoderStream");
    return new TextDecoderStreamBefore();
  };
  TextDecoder.prototype.decode = function () {
    touched.push("decode");
    return decode.apply(this, arguments);
  };
  ReadableStreamDefaultController.prototype.enqueue = function () {
    touched.push("enqueue");
    return enqueue.apply(this, arguments);
  };
  let patched;
  try {
    patched = await read(new Blob(["patched"]).textStream());
  } finally {
    ReadableStream.prototype.pipeThrough = pipeThrough;
    globalThis.TextDecoderStream = TextDecoderStreamBefore;
    TextDecoder.prototype.decode = decode;
    ReadableStreamDefaultController.prototype.enqueue = enqueue;
  }
  out.patched = [patched, touched];
  out.cancel = await new Blob(["abc"])
    .textStream()
    .cancel("why")
    .then(
      () => "fulfilled",
      (error) => String(error),
    );
  return JSON.stringify(out);
})();
