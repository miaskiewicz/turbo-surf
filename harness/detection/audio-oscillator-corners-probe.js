(async () => {
  const result = {};
  for (const [name, config] of Object.entries({
    default: {},
    oneHz: { frequency: 1 },
    twoHz: { frequency: 2 },
    reverse: { frequency: -220 },
    delayedRamp: { frequency: 440, start: 0.005, ramp: true },
    kRateDetune: { frequency: 440, kRate: true, detune: 599 },
    detuneRamp: { frequency: 440, detuneRamp: true },
  })) {
    const c = new OfflineAudioContext(1, 2048, 44100),
      s = c.createOscillator();
    if (config.frequency !== undefined) s.frequency.value = config.frequency;
    if (config.kRate) s.frequency.automationRate = "k-rate";
    if (config.detune !== undefined) s.detune.value = config.detune;
    if (config.ramp) {
      s.frequency.setValueAtTime(220, 0);
      s.frequency.linearRampToValueAtTime(880, 0.03);
    }
    if (config.detuneRamp) {
      s.detune.setValueAtTime(0, 0);
      s.detune.linearRampToValueAtTime(599, 0.02);
    }
    s.connect(c.destination);
    s.start(config.start || 0);
    const output = await c.startRendering();
    result[name] = {
      pcm: Array.from(output.getChannelData(0)),
      frequency: s.frequency.value,
      detune: s.detune.value,
    };
  }
  return result;
})();
