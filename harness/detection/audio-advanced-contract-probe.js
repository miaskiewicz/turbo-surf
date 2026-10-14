(async () => {
  const result = { errors: {}, graphs: {}, shapes: {} };
  const context = new OfflineAudioContext(1, 2048, 44100),
    source = context.createBufferSource(),
    osc = context.createOscillator(),
    gain = context.createGain(),
    comp = context.createDynamicsCompressor();
  const checks = {
    bufferARate: () => (source.playbackRate.automationRate = "a-rate"),
    compressorARate: () => (comp.threshold.automationRate = "a-rate"),
    oscKRate: () => (osc.frequency.automationRate = "k-rate"),
    zeroExp: () => gain.gain.exponentialRampToValueAtTime(0, 0.1),
    shortCurve: () => gain.gain.setValueCurveAtTime(new Float32Array([1]), 0, 0.1),
    emptyCurve: () => gain.gain.setValueCurveAtTime(new Float32Array(), 0, 0.1),
    curveNonfinite: () => gain.gain.setValueCurveAtTime(new Float32Array([1, NaN]), 0, 0.1),
    curveOverlap: () => {
      gain.gain.setValueCurveAtTime(new Float32Array([1, 2]), 0, 0.1);
      gain.gain.setValueAtTime(2, 0.05);
    },
    bufferDoubleSet: () => {
      source.buffer = context.createBuffer(1, 1, 44100);
      source.buffer = context.createBuffer(1, 1, 44100);
    },
    delayMax0: () => context.createDelay(0),
    shaperPlainArray: () => {
      context.createWaveShaper().curve = [0, 1];
    },
  };
  for (const [name, fn] of Object.entries(checks))
    try {
      fn();
      result.errors[name] = null;
    } catch (error) {
      result.errors[name] = { name: error.name, message: error.message };
    }
  for (const name of ["BiquadFilterNode", "DelayNode", "WaveShaperNode", "StereoPannerNode"]) {
    const node = new globalThis[name](context);
    result.shapes[name] = {
      brand: Object.prototype.toString.call(node),
      keys: Object.getOwnPropertyNames(globalThis[name].prototype),
      params: {},
    };
    for (const param of ["frequency", "Q", "gain", "detune", "delayTime", "pan"])
      if (node[param]) {
        const p = node[param];
        result.shapes[name].params[param] = {
          value: p.value,
          min: p.minValue,
          max: p.maxValue,
          rate: p.automationRate,
        };
      }
  }
  for (const kind of [
    "cancel",
    "holdLinear",
    "holdCurve",
    "holdTarget",
    "kRateGain",
    "negativeRate",
    "scheduledRate",
  ]) {
    const c = new OfflineAudioContext(1, 2048, 44100),
      b = c.createBuffer(1, 1024, 44100),
      d = b.getChannelData(0);
    for (let i = 0; i < d.length; i++) d[i] = Math.sin(i * 0.13) * 0.4 + ((i % 67) - 33) / 200;
    const s = c.createBufferSource();
    s.buffer = b;
    const g = c.createGain();
    s.connect(g);
    g.connect(c.destination);
    if (kind === "cancel" || kind === "holdLinear") {
      g.gain.setValueAtTime(0, 0);
      g.gain.linearRampToValueAtTime(1, 0.04);
      g.gain[kind === "cancel" ? "cancelScheduledValues" : "cancelAndHoldAtTime"](0.012);
    }
    if (kind === "holdCurve") {
      g.gain.setValueCurveAtTime(new Float32Array([0, 0.6, 0.2, 1]), 0, 0.03);
      g.gain.cancelAndHoldAtTime(0.012);
    }
    if (kind === "holdTarget") {
      g.gain.setValueAtTime(0.2, 0);
      g.gain.setTargetAtTime(0.9, 0.004, 0.005);
      g.gain.cancelAndHoldAtTime(0.012);
    }
    if (kind === "kRateGain") {
      g.gain.automationRate = "k-rate";
      g.gain.setValueAtTime(0, 0);
      g.gain.linearRampToValueAtTime(1, 0.03);
    }
    if (kind === "negativeRate") s.playbackRate.value = -0.5;
    if (kind === "scheduledRate") {
      s.playbackRate.setValueAtTime(0.5, 0);
      s.playbackRate.linearRampToValueAtTime(2, 0.03);
    }
    try {
      s.start(0, kind === "negativeRate" ? 0.02 : 0);
      const out = await c.startRendering();
      result.graphs[kind] = { pcm: Array.from(out.getChannelData(0)), time: c.currentTime };
    } catch (error) {
      result.graphs[kind] = { error: { name: error.name, message: error.message } };
    }
  }
  return result;
})();
