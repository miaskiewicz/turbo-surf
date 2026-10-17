/* eslint-disable no-await-in-loop -- Sequential source-pull and endpoint phases are the measured behavior. */
(async () => {
  const out = {};
  for (const order of [
    "bufferFirst",
    "streamFirst",
    "portFirst",
    "streamFirstPort",
    "detachLaterBuffer",
  ]) {
    let observed,
      nestedError = null,
      phase = false;
    const buffer = new ArrayBuffer(8),
      channel = new MessageChannel();
    const source = new ReadableStream(
      {
        pull(c) {
          observed = { buffer: buffer.byteLength, phase };
          try {
            if (order === "portFirst" || order === "streamFirstPort") {
              const copy = structuredClone(channel.port1, { transfer: [channel.port1] });
              copy.close();
            }
            if (order === "detachLaterBuffer") structuredClone(buffer, { transfer: [buffer] });
          } catch (e) {
            nestedError = e.name;
          }
          c.close();
        },
      },
      { highWaterMark: 0 },
    );
    await Promise.resolve();
    await Promise.resolve();
    let error = null;
    const transfer =
      order === "bufferFirst"
        ? [buffer, source]
        : order === "portFirst"
          ? [channel.port1, source]
          : order === "streamFirstPort"
            ? [source, channel.port1]
            : [source, buffer];
    try {
      structuredClone(source, { transfer });
    } catch (e) {
      error = e.name;
    }
    phase = true;
    await new Promise((r) => setTimeout(r, 10));
    channel.port1.close();
    channel.port2.close();
    out[order] = { observed, nestedError, error, buffer: buffer.byteLength, locked: source.locked };
  }
  return out;
})();
