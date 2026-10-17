(async () => {
  const context = new OfflineAudioContext(1, 128, 44100);
  const values = (array) => Array.from(array, (value) => (Number.isNaN(value) ? "NaN" : value));
  const error = (callback) => {
    try {
      callback();
      return null;
    } catch (caught) {
      return { name: caught.name, dom: caught instanceof DOMException, code: caught.code ?? null };
    }
  };
  const frequencies = new Float32Array([
    -1,
    0,
    10,
    100,
    350,
    1000,
    10000,
    22050,
    22051,
    NaN,
    Infinity,
  ]);
  const graphs = {};
  for (const type of [
    "lowpass",
    "highpass",
    "bandpass",
    "lowshelf",
    "highshelf",
    "peaking",
    "notch",
    "allpass",
  ]) {
    const node = context.createBiquadFilter();
    node.type = type;
    node.frequency.value = 1200;
    node.Q.value = 0.7;
    node.gain.value = 3;
    node.detune.value = 120;
    const magnitude = new Float32Array(frequencies.length),
      phase = new Float32Array(frequencies.length);
    const returned = node.getFrequencyResponse(frequencies, magnitude, phase);
    graphs[type] = {
      magnitude: values(magnitude),
      phase: values(phase),
      undefined: returned === undefined,
    };
  }
  const node = context.createBiquadFilter();
  node.frequency.value = 1200;
  node.Q.value = 0.7;
  const input = new Float32Array([10, 350, 1000]);
  const buffer = new ArrayBuffer(16),
    magnitude = new Float32Array(buffer, 0, 3),
    phase = new Float32Array(buffer, 4, 3);
  node.getFrequencyResponse(input, magnitude, phase);
  const same = new Float32Array([10, 350, 1000]);
  node.getFrequencyResponse(same, same, same);
  const method = BiquadFilterNode.prototype.getFrequencyResponse;
  const errors = {
    forged: error(() => method.call({}, input, magnitude, phase)),
    missing: error(() => node.getFrequencyResponse(input, magnitude)),
    plain: error(() => node.getFrequencyResponse([10, 350, 1000], magnitude, phase)),
    wide: error(() => node.getFrequencyResponse(new Float64Array(3), magnitude, phase)),
    mismatch: error(() => node.getFrequencyResponse(input, new Float32Array(2), phase)),
    empty: error(() =>
      node.getFrequencyResponse(new Float32Array(0), new Float32Array(0), new Float32Array(0)),
    ),
  };
  return {
    graphs,
    overlap: values(new Float32Array(buffer)),
    same: values(same),
    errors,
    shape: { name: method.name, length: method.length },
  };
})();
