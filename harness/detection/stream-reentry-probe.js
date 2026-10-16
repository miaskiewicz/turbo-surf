(async () => {
  let returned = false;
  const events = [];
  const later = new ReadableStream({}, { highWaterMark: 0 });
  const first = new ReadableStream(
    {
      pull(controller) {
        let error = null;
        try {
          later.getReader();
        } catch (value) {
          error = value.name;
        }
        events.push({ returned, error });
        controller.close();
      },
    },
    { highWaterMark: 0 },
  );
  await Promise.resolve();
  await Promise.resolve();
  const buffer = new ArrayBuffer(8);
  let error = null;
  let copy;
  try {
    copy = structuredClone({ first, later }, { transfer: [buffer, first, later] });
  } catch (value) {
    error = value.name;
  }
  returned = true;
  if (copy) await copy.first.getReader().read();
  await new Promise((resolve) => setTimeout(resolve, 10));
  return { error, events, bufferBytes: buffer.byteLength, locks: [first.locked, later.locked] };
})();
