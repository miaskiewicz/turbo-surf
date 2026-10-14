(async () => {
  const result = { graphs: {}, contracts: {} };
  const cases = {
    delayedOscillator: { wave: "sine", frequency: 440, start: 0.005, stop: 0.015 },
    gainRamp: {
      wave: "sine",
      frequency: 880,
      automation: {
        gain: [
          ["setValueAtTime", 0, 0],
          ["linearRampToValueAtTime", 0.75, 0.01],
          ["linearRampToValueAtTime", 0.25, 0.03],
        ],
      },
    },
    exponentialRamp: {
      wave: "triangle",
      frequency: 330,
      automation: {
        gain: [
          ["setValueAtTime", 0.1, 0],
          ["exponentialRampToValueAtTime", 0.8, 0.02],
        ],
      },
    },
    target: {
      wave: "square",
      frequency: 220,
      automation: {
        gain: [
          ["setValueAtTime", 0.2, 0],
          ["setTargetAtTime", 0.9, 0.004, 0.005],
        ],
      },
    },
    curve: {
      wave: "sine",
      frequency: 440,
      automation: { gain: [["setValueCurveAtTime", [0.1, 0.6, 0.2, 0.8], 0.002, 0.02]] },
    },
    frequencyRamp: {
      wave: "sine",
      frequency: 440,
      automation: {
        frequency: [
          ["setValueAtTime", 220, 0],
          ["linearRampToValueAtTime", 880, 0.03],
        ],
      },
    },
    detunedOscillator: { wave: "sine", frequency: 440, detune: 600 },
    offsetDuration: { buffer: true, start: 0.005, offset: 0.002, duration: 0.012 },
    doubleRate: { buffer: true, rate: 2 },
    halfRate: { buffer: true, rate: 0.5 },
    detunedBuffer: { buffer: true, detune: 600 },
    loop: { buffer: true, loop: true, loopStart: 0.004, loopEnd: 0.01 },
    resample: { buffer: true, inputRate: 32000 },
    lowpass: { buffer: true, biquad: { type: "lowpass", frequency: 1200, Q: 0.707 } },
    highpass: { buffer: true, biquad: { type: "highpass", frequency: 800, Q: 2 } },
    waveshaper: { buffer: true, curve: [-1, -0.2, 0, 0.2, 1] },
    delay: { buffer: true, delay: 0.003 },
    pan: { buffer: true, pan: 0.4, channels: 2 },
  };
  for (const [name, c] of Object.entries(cases))
    try {
      const context = new OfflineAudioContext(c.channels || 1, 2048, 44100);
      let source;
      if (c.buffer) {
        const input = context.createBuffer(1, 1024, c.inputRate || 44100),
          data = input.getChannelData(0);
        for (let i = 0; i < data.length; i++)
          data[i] = Math.sin(i * 0.13) * 0.4 + ((i % 67) - 33) / 200;
        source = context.createBufferSource();
        source.buffer = input;
        if (c.rate !== undefined) source.playbackRate.value = c.rate;
        if (c.detune !== undefined) source.detune.value = c.detune;
        if (c.loop) {
          source.loop = true;
          source.loopStart = c.loopStart;
          source.loopEnd = c.loopEnd;
        }
      } else {
        source = context.createOscillator();
        source.type = c.wave;
        source.frequency.value = c.frequency;
        if (c.detune !== undefined) source.detune.value = c.detune;
      }
      let tail = source,
        gain = context.createGain();
      tail.connect(gain);
      tail = gain;
      if (c.automation)
        for (const [param, events] of Object.entries(c.automation))
          for (const [method, ...args] of events)
            (param === "gain" ? gain.gain : source[param])[method](
              ...args.map((value) => (Array.isArray(value) ? new Float32Array(value) : value)),
            );
      if (c.biquad) {
        const node = context.createBiquadFilter();
        node.type = c.biquad.type;
        node.frequency.value = c.biquad.frequency;
        node.Q.value = c.biquad.Q;
        tail.connect(node);
        tail = node;
      }
      if (c.curve) {
        const node = context.createWaveShaper();
        node.curve = new Float32Array(c.curve);
        tail.connect(node);
        tail = node;
      }
      if (c.delay) {
        const node = context.createDelay();
        node.delayTime.value = c.delay;
        tail.connect(node);
        tail = node;
      }
      if (c.pan) {
        const node = context.createStereoPanner();
        node.pan.value = c.pan;
        tail.connect(node);
        tail = node;
      }
      tail.connect(context.destination);
      if (c.buffer && c.offset !== undefined) source.start(c.start || 0, c.offset, c.duration);
      else source.start(c.start || 0);
      if (c.stop) source.stop(c.stop);
      const rendered = await context.startRendering(),
        pcm = Array.from({ length: rendered.numberOfChannels }, (_, i) =>
          Array.from(rendered.getChannelData(i)),
        );
      let hash = 0x811c9dc5,
        energy = 0;
      for (const channel of pcm)
        for (const x of channel) {
          const n = Math.round(x * 1e7);
          hash = Math.imul(hash ^ (n & 255), 0x01000193) >>> 0;
          hash = Math.imul(hash ^ ((n >>> 8) & 255), 0x01000193) >>> 0;
          energy += Math.abs(x);
        }
      result.graphs[name] = {
        pcm,
        hash: hash.toString(16).padStart(8, "0"),
        energy,
        firstNonzero: pcm[0].findIndex((value) => value !== 0),
        time: context.currentTime,
      };
    } catch (error) {
      result.graphs[name] = { error: { name: error.name, message: error.message } };
    }
  return result;
})();
