(async () => {
  const result = { graphs: {}, contracts: {} };
  const failure = (error) => ({ name: error.name, message: error.message });
  const graph = async ({ wave, frequency, compression, gain, buffer }) => {
    const context = new OfflineAudioContext(1, 44100, 44100);
    let source;
    if (buffer) {
      const input = context.createBuffer(1, 44100, 44100);
      const channel = input.getChannelData(0);
      for (let i = 0; i < channel.length; i++)
        channel[i] = Math.sin((2 * Math.PI * frequency * i) / context.sampleRate);
      source = context.createBufferSource();
      source.buffer = input;
    } else {
      source = context.createOscillator();
      source.type = wave;
      source.frequency.value = frequency;
    }
    let tail = source;
    if (gain != null) {
      const node = context.createGain();
      node.gain.value = gain;
      tail.connect(node);
      tail = node;
    }
    if (compression) {
      const node = context.createDynamicsCompressor();
      for (const [name, value] of Object.entries(compression)) node[name].value = value;
      tail.connect(node);
      tail = node;
    }
    tail.connect(context.destination);
    source.start(0);
    const stateBefore = context.state;
    const promise = context.startRendering();
    const promiseBrand = Object.prototype.toString.call(promise);
    const rendered = await promise;
    const pcm = rendered.getChannelData(0);
    let hash = 0x811c9dc5;
    let energy = 0;
    for (const sample of pcm) {
      const quantized = Math.round(sample * 1e7);
      hash = Math.imul(hash ^ (quantized & 255), 0x01000193) >>> 0;
      hash = Math.imul(hash ^ ((quantized >>> 8) & 255), 0x01000193) >>> 0;
      energy += Math.abs(sample);
    }
    return {
      samples: pcm.length,
      sampleRate: rendered.sampleRate,
      channels: rendered.numberOfChannels,
      energy,
      hash: hash.toString(16).padStart(8, "0"),
      pcm: Array.from(pcm),
      stateBefore,
      stateAfter: context.state,
      currentTime: context.currentTime,
      promiseBrand,
      stableChannelView: rendered.getChannelData(0) === pcm,
      sourceContext: source.context === context,
      brands: [context, source, rendered, pcm].map((value) =>
        Object.prototype.toString.call(value),
      ),
    };
  };
  const fingerprintCompressor = { threshold: -50, knee: 40, ratio: 12, attack: 0, release: 0.25 };
  const graphs = {
    triangle: { wave: "triangle", frequency: 10000 },
    compressor: { buffer: true, frequency: 1000, compression: fingerprintCompressor },
    fingerprint: { wave: "triangle", frequency: 10000, compression: fingerprintCompressor },
    sine: { wave: "sine", frequency: 440 },
    squareGain: { wave: "square", frequency: 220, gain: 0.25 },
    alternateCompression: {
      wave: "sine",
      frequency: 880,
      compression: { threshold: -24, knee: 20, ratio: 4, attack: 0.01, release: 0.1 },
    },
  };
  for (const [name, description] of Object.entries(graphs)) {
    try {
      result.graphs[name] = await graph(description);
    } catch (error) {
      result.graphs[name] = { error: failure(error) };
    }
  }
  const attempts = {
    withoutNew: () => OfflineAudioContext(1, 128, 44100),
    zeroChannels: () => new OfflineAudioContext(0, 128, 44100),
    zeroLength: () => new OfflineAudioContext(1, 0, 44100),
    lowSampleRate: () => new OfflineAudioContext(1, 128, 1000),
    foreignBufferReceiver: () => AudioBuffer.prototype.getChannelData.call({}, 0),
    repeatedStart: async () => {
      const context = new OfflineAudioContext(1, 128, 44100);
      await context.startRendering();
      return context.startRendering();
    },
    crossContextConnection: () => {
      const a = new OfflineAudioContext(1, 128, 44100);
      const b = new OfflineAudioContext(1, 128, 44100);
      return a.createOscillator().connect(b.destination);
    },
  };
  for (const [name, attempt] of Object.entries(attempts)) {
    try {
      const value = attempt();
      const promise = value instanceof Promise;
      await value;
      result.contracts[name] = { promise, success: true };
    } catch (error) {
      result.contracts[name] = { error: failure(error) };
    }
  }
  return result;
})();
