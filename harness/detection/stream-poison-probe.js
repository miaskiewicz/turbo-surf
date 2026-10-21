/* eslint-disable unicorn/consistent-function-scoping, no-extend-native -- The oracle is a self-contained expression evaluated in the page realm. Public prototype replacement is the adversarial test input. */
(async () => {
  const source = new ReadableStream({
    start(c) {
      c.enqueue(17);
      c.close();
    },
  });
  const originalChannel = MessageChannel,
    originalPortPost = MessagePort.prototype.postMessage;
  const originalPipe = ReadableStream.prototype.pipeTo;
  const bad = () => {
    throw new Error("page replacement reached");
  };
  let copy;
  try {
    globalThis.MessageChannel = bad;
    MessagePort.prototype.postMessage = bad;
    ReadableStream.prototype.pipeTo = bad;
    copy = structuredClone(source, { transfer: [source] });
  } finally {
    globalThis.MessageChannel = originalChannel;
    MessagePort.prototype.postMessage = originalPortPost;
    ReadableStream.prototype.pipeTo = originalPipe;
  }
  const reader = copy.getReader(),
    first = await reader.read(),
    last = await reader.read();
  return JSON.stringify([source.locked, first.value, last.done]);
})();
