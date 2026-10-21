(async () => {
  const error = async (callback) => {
    try {
      await callback();
      return null;
    } catch (e) {
      return { name: e.name, message: e.message };
    }
  };
  const shadow = new OfflineAudioContext(1, 128, 44100),
    buffer = shadow.createBuffer(1, 128, 44100),
    source = shadow.createBufferSource(),
    gain = shadow.createGain();
  buffer.getChannelData(0).fill(0.25);
  source.buffer = buffer;
  source.connect(gain);
  gain.gain.value = 2;
  gain.connect(shadow.destination);
  Object.defineProperty(gain.gain, "value", { value: 100 });
  source.start();
  const shadowResult = (await shadow.startRendering()).getChannelData(0)[0];
  const direct = new OfflineAudioContext(1, 128, 44100),
    input = direct.createBuffer(1, 128, 44100),
    view = input.getChannelData(0);
  view.fill(0.25);
  view[Symbol.iterator] = function* () {
    for (let i = 0; i < 128; i++) yield 9;
  };
  const node = direct.createBufferSource();
  node.buffer = input;
  node.connect(direct.destination);
  node.start();
  const iteratorResult = (await direct.startRendering()).getChannelData(0)[0];
  const transfers = new OfflineAudioContext(1, 128, 44100),
    transferred = transfers.createBuffer(1, 128, 44100),
    samples = transferred.getChannelData(0);
  samples.fill(0.25);
  let transferError = await error(() =>
    structuredClone(samples.buffer, { transfer: [samples.buffer] }),
  );
  const chan = transferred.getChannelData(0),
    source2 = transfers.createBufferSource();
  source2.buffer = transferred;
  source2.connect(transfers.destination);
  source2.start();
  let transferredPCM;
  let transferredResult = await error(async () => {
    transferredPCM = (await transfers.startRendering()).getChannelData(0)[0];
  });
  const ctx = new OfflineAudioContext(1, 128, 44100),
    audio = ctx.createBuffer(1, 128, 44100);
  const copyPast = await error(() => audio.copyToChannel(new Float32Array([1]), 0, 999));
  const events = [],
    osc = ctx.createOscillator();
  osc.start();
  osc.stop(0.001);
  osc.connect(ctx.destination);
  osc.onended = (e) => events.push(["ended", e.isTrusted, ctx.state]);
  ctx.onstatechange = (e) => events.push(["state", e.isTrusted, ctx.state]);
  ctx.oncomplete = (e) =>
    events.push(["complete", e.isTrusted, ctx.state, Object.prototype.toString.call(e)]);
  await ctx.startRendering();
  await new Promise((resolve) => setTimeout(resolve, 10));
  return {
    shadowResult,
    iteratorResult,
    transferError,
    channelAfterTransferLength: chan.length,
    originalAfterTransferLength: samples.length,
    transferredResult,
    transferredPCM,
    copyPast,
    events,
    params: {
      compressor: ctx.createDynamicsCompressor().threshold.automationRate,
      buffer: ctx.createBufferSource().playbackRate.automationRate,
      osc: ctx.createOscillator().frequency.automationRate,
    },
    webkitOffline: typeof webkitOfflineAudioContext,
  };
})();
