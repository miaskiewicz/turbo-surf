// Native offline Web Audio records and DSP; no public host/core references.
(function (core, make) {
  "use strict";
  const ops = core.ops,
    get = ops.op_interface_record_get,
    set = ops.op_interface_record_set;
  const define = Object.defineProperty,
    create = Object.create,
    apply = Reflect.apply;
  const FloatArray = Float32Array,
    LocalPromise = Promise,
    LocalTypeError = TypeError,
    LocalRangeError = RangeError;
  const LocalDOMException = DOMException;
  const LocalArray = Array;
  const LocalNumber = Number,
    LocalString = String,
    LocalBoolean = Boolean;
  const numberFinite = Number.isFinite,
    fround = Math.fround,
    minimum = Math.min,
    maximum = Math.max,
    ceiling = Math.ceil;
  const setPrototype = Object.setPrototypeOf;
  const arrayIncludes = Array.prototype.includes,
    arrayPush = Array.prototype.push,
    arraySort = Array.prototype.sort;
  const typedSet = Float32Array.prototype.set,
    typedSubarray = Float32Array.prototype.subarray;
  const arrayFrom = Array.from;
  const promiseResolve = Promise.resolve,
    promiseReject = Promise.reject,
    promiseThen = Promise.prototype.then;
  const LocalEvent = Event;
  const eventAdd = EventTarget.prototype.addEventListener,
    eventRemove = EventTarget.prototype.removeEventListener;
  function array(length) {
    const shape = create(null);
    shape.length = length;
    return apply(arrayFrom, LocalArray, [shape]);
  }
  function map(values, callback) {
    const result = array(values.length);
    for (let index = 0; index < values.length; index++)
      result[index] = callback(values[index], index);
    return result;
  }
  const includes = (values, value) => apply(arrayIncludes, values, [value]);
  function filter(values, callback) {
    const result = array(values.length);
    let count = 0;
    for (let index = 0; index < values.length; index++)
      if (callback(values[index])) result[count++] = values[index];
    result.length = count;
    setPrototype(result, null);
    return result;
  }
  const rejected = (value) => apply(promiseReject, LocalPromise, [value]);
  function samples(channel, length) {
    // Indexed integer reads bypass guest iterators and typed-array expandos.
    const result = array(length);
    const available = apply(typedLength, channel, []);
    for (let index = 0; index < length; index++)
      result[index] = index < available ? channel[index] : 0;
    return result;
  }

  const typedLength = Object.getOwnPropertyDescriptor(
    Object.getPrototypeOf(Float32Array.prototype),
    "length",
  ).get;
  const typedArrayTag = Object.getOwnPropertyDescriptor(
    Object.getPrototypeOf(Float32Array.prototype),
    Symbol.toStringTag,
  ).get;
  const dispatchHost = ops.op_dispatch_host_event;
  const prototypes = create(null),
    constructors = create(null);
  const failure = (message, name = "NotSupportedError") => new LocalDOMException(message, name);
  const unsupported = () => {
    throw failure("This offline audio operation is not supported by the native renderer");
  };
  function record(object, kinds) {
    const value = get(object);
    if (!value || !value.audio || (kinds && !includes(kinds, value.kind)))
      throw new LocalTypeError("Illegal invocation");
    return value;
  }
  function object(kind, state) {
    const result = ops.op_audio_object(
      prototypes[kind],
      kind !== "AudioBuffer" && kind !== "AudioParam",
    );
    setPrototype(state, null);
    if (state.inputs) setPrototype(state.inputs, null);
    if (state.events) setPrototype(state.events, null);
    if (state.nodes) setPrototype(state.nodes, null);
    if (state.channels) setPrototype(state.channels, null);
    state.audio = true;
    state.kind = kind;
    set(result, state);
    return result;
  }
  function install(name, parent, length, initialize) {
    const initializeSource =
      initialize ||
      function () {
        throw new LocalTypeError("Illegal constructor");
      };
    // Construct through Reflect.construct in the private native callback. The
    // proxy distinguishes new from calls without accepting forged receivers.
    const source = new Proxy(initializeSource, {
      apply() {
        throw new LocalTypeError(
          initialize
            ? "Failed to construct '" +
                name +
                "': Please use the 'new' operator, this DOM object constructor cannot be called as a function."
            : "Illegal constructor",
        );
      },
      construct(target, args, newTarget) {
        const result = apply(target, create(newTarget.prototype), args);
        if (result && typeof result === "object") setPrototype(result, newTarget.prototype);
        return result;
      },
    });
    const constructor = make(source, name, length, true, true, false);
    const prototype = create(parent || Object.prototype);
    define(prototype, "constructor", { value: constructor, writable: true, configurable: true });
    define(prototype, Symbol.toStringTag, { value: name, configurable: true });
    constructor.prototype = prototype;
    define(constructor, "prototype", { writable: false });
    if (parent && parent.constructor) setPrototype(constructor, parent.constructor);
    define(globalThis, name, { value: constructor, writable: true, configurable: true });
    prototypes[name] = prototype;
    constructors[name] = constructor;
    return prototype;
  }
  function method(kind, name, length, implementation) {
    define(prototypes[kind], name, {
      value: make(implementation, name, length),
      writable: true,
      enumerable: true,
      configurable: true,
    });
  }
  function getter(kind, name, implementation, setter) {
    const descriptor = {
      get: make(implementation, "get " + name, 0),
      enumerable: true,
      configurable: true,
    };
    if (setter) descriptor.set = make(setter, "set " + name, 1);
    define(prototypes[kind], name, descriptor);
  }
  function required(count, args, name) {
    if (args.length < count)
      throw new LocalTypeError(
        "Failed to execute '" +
          name +
          "': " +
          count +
          " argument(s) required, but only " +
          args.length +
          " present.",
      );
  }
  function finite(value) {
    const converted = LocalNumber(value);
    if (!numberFinite(converted)) throw new LocalTypeError("The provided value is non-finite.");
    return converted;
  }
  function validate(channels, length, sampleRate) {
    if (channels < 1 || channels > 32)
      throw failure(
        "Failed to construct 'OfflineAudioContext': The number of channels provided (" +
          channels +
          ") is outside the range [1, 32].",
      );
    if (length < 1)
      throw failure(
        "Failed to construct 'OfflineAudioContext': The number of frames provided (" +
          length +
          ") is less than the minimum bound (1).",
      );
    if (sampleRate < 3000 || sampleRate > 768000)
      throw failure(
        "Failed to construct 'OfflineAudioContext': The sample rate provided (" +
          sampleRate +
          ") is outside the range [3000, 768000].",
      );
    if (channels * length > 262144)
      throw failure("Offline audio resource limit exceeded", "QuotaExceededError");
  }
  function makeBuffer(channels, length, rate, data) {
    validate(channels, length, rate);
    const buffers = array(channels);
    for (let index = 0; index < channels; index++) {
      const channel = new FloatArray(length);
      if (data) apply(typedSet, channel, [data[index]]);
      buffers[index] = channel;
    }
    return object("AudioBuffer", { channels: buffers, length, sampleRate: rate });
  }
  function node(context, kind) {
    const owner = record(context, ["OfflineAudioContext"]);
    if (owner.nodes.length >= 64)
      throw failure("Offline audio node limit exceeded", "QuotaExceededError");
    const state = {
      context,
      id: owner.nodes.length,
      inputs: [],
      parameters: create(null),
      started: false,
      stopped: null,
    };
    const result = object(kind, state);
    apply(arrayPush, owner.nodes, [result]);
    return result;
  }
  function param(node, name, value, minimum, maximum) {
    const state = record(node);
    const result = object("AudioParam", {
      node,
      name,
      value: fround(value),
      events: [],
      inputs: [],
      defaultValue: fround(value),
      minValue: fround(minimum),
      maxValue: fround(maximum),
      automationRate:
        state.kind === "DynamicsCompressorNode" || state.kind === "AudioBufferSourceNode"
          ? "k-rate"
          : "a-rate",
    });
    state.parameters[name] = result;
    return result;
  }
  const nodeKinds = [
    "OscillatorNode",
    "GainNode",
    "DynamicsCompressorNode",
    "AudioBufferSourceNode",
    "AudioDestinationNode",
    "BiquadFilterNode",
    "DelayNode",
    "WaveShaperNode",
    "StereoPannerNode",
  ];
  install("BaseAudioContext", EventTarget.prototype, 0);
  install(
    "OfflineAudioContext",
    prototypes.BaseAudioContext,
    1,
    function (channels, length, sampleRate) {
      if (channels && typeof channels === "object") {
        const options = channels;
        channels = options.numberOfChannels ?? 1;
        length = options.length;
        sampleRate = options.sampleRate;
      } else required(3, arguments, "OfflineAudioContext");
      channels = finite(channels) >>> 0;
      length = finite(length) >>> 0;
      sampleRate = fround(finite(sampleRate));
      validate(channels, length, sampleRate);
      const context = object("OfflineAudioContext", {
        channels,
        length,
        sampleRate,
        state: "suspended",
        time: 0,
        nodes: [],
        rendering: false,
      });
      const state = record(context);
      state.destination = node(context, "AudioDestinationNode");
      return context;
    },
  );
  install("AudioContext", prototypes.BaseAudioContext, 0, function () {
    throw failure("The native renderer has no realtime audio device backend");
  });
  define(globalThis, "webkitAudioContext", {
    value: constructors.AudioContext,
    writable: true,
    configurable: true,
  });
  install("AudioBuffer", null, 1, function (options) {
    required(1, arguments, "AudioBuffer");
    return makeBuffer(
      finite(options.numberOfChannels ?? 1) >>> 0,
      finite(options.length) >>> 0,
      fround(finite(options.sampleRate)),
    );
  });
  install("OfflineAudioCompletionEvent", Event.prototype, 1, function (type, options = {}) {
    required(1, arguments, "OfflineAudioCompletionEvent");
    if (options.renderedBuffer != null) record(options.renderedBuffer, ["AudioBuffer"]);
    const event = new LocalEvent(LocalString(type), options);
    set(event, {
      audio: true,
      kind: "OfflineAudioCompletionEvent",
      renderedBuffer: options.renderedBuffer ?? null,
    });
    return event;
  });
  getter("OfflineAudioCompletionEvent", "renderedBuffer", function () {
    return record(this, ["OfflineAudioCompletionEvent"]).renderedBuffer;
  });
  install("AudioNode", EventTarget.prototype, 0);
  install("AudioScheduledSourceNode", prototypes.AudioNode, 0);
  install("AudioParam", null, 0);
  install("AudioDestinationNode", prototypes.AudioNode, 0);
  function oscillator(context, options = {}) {
    options ??= {};
    const owner = record(context, ["OfflineAudioContext"]);
    const result = node(context, "OscillatorNode");
    param(result, "frequency", 440, -owner.sampleRate / 2, owner.sampleRate / 2);
    param(result, "detune", 0, -153600, 153600);
    record(result).type = "sine";
    const type = options.type,
      frequency = options.frequency,
      detune = options.detune;
    if (type !== undefined) result.type = type;
    if (frequency !== undefined) result.frequency.value = frequency;
    if (detune !== undefined) result.detune.value = detune;
    return result;
  }
  install("OscillatorNode", prototypes.AudioScheduledSourceNode, 1, function (context, options) {
    required(1, arguments, "OscillatorNode");
    return oscillator(context, options);
  });
  function gain(context, options = {}) {
    options ??= {};
    const result = node(context, "GainNode");
    param(result, "gain", 1, -3.4028234663852886e38, 3.4028234663852886e38);
    const value = options.gain;
    if (value !== undefined) result.gain.value = value;
    return result;
  }
  install("GainNode", prototypes.AudioNode, 1, function (context, options) {
    required(1, arguments, "GainNode");
    return gain(context, options);
  });
  function compressor(context, options = {}) {
    options ??= {};
    const result = node(context, "DynamicsCompressorNode");
    record(result).reduction = 0;
    for (const [name, value, minimum, maximum] of [
      ["threshold", -24, -100, 0],
      ["knee", 30, 0, 40],
      ["ratio", 12, 1, 20],
      ["attack", 0.003, 0, 1],
      ["release", 0.25, 0, 1],
    ]) {
      param(result, name, value, minimum, maximum);
      const option = options[name];
      if (option !== undefined) record(result).parameters[name].value = option;
    }
    return result;
  }
  install("DynamicsCompressorNode", prototypes.AudioNode, 1, function (context, options) {
    required(1, arguments, "DynamicsCompressorNode");
    return compressor(context, options);
  });
  function bufferSource(context, options = {}) {
    options ??= {};
    const result = node(context, "AudioBufferSourceNode");
    param(result, "playbackRate", 1, -3.4028234663852886e38, 3.4028234663852886e38);
    param(result, "detune", 0, -153600, 153600);
    const state = record(result);
    state.buffer = null;
    state.bufferSet = false;
    state.loop = false;
    state.loopStart = 0;
    state.loopEnd = 0;
    for (const key of ["buffer", "loop", "loopStart", "loopEnd"]) {
      const value = options[key];
      if (value !== undefined) result[key] = value;
    }
    return result;
  }
  install(
    "AudioBufferSourceNode",
    prototypes.AudioScheduledSourceNode,
    1,
    function (context, options) {
      required(1, arguments, "AudioBufferSourceNode");
      return bufferSource(context, options);
    },
  );
  function biquad(context, options = {}) {
    options ??= {};
    const owner = record(context, ["OfflineAudioContext"]),
      result = node(context, "BiquadFilterNode");
    record(result).type = "lowpass";
    for (const [name, value, min, max] of [
      ["frequency", 350, 0, owner.sampleRate / 2],
      ["Q", 1, -3.4028234663852886e38, 3.4028234663852886e38],
      ["gain", 0, -3.4028234663852886e38, 1541.273681640625],
      ["detune", 0, -153600, 153600],
    ]) {
      param(result, name, value, min, max);
      const option = options[name];
      if (option !== undefined) record(result).parameters[name].value = option;
    }
    const type = options.type;
    if (type !== undefined) result.type = type;
    return result;
  }
  function delay(context, maximumDelay = 1, options = {}) {
    maximumDelay = finite(maximumDelay);
    const owner = record(context, ["OfflineAudioContext"]);
    if (maximumDelay <= 0 || maximumDelay >= 180)
      throw failure(
        "maxDelayTime must be greater than zero and less than 180",
        "NotSupportedError",
      );
    if (maximumDelay * owner.sampleRate > 1000000)
      throw failure("Native delay allocation limit exceeded", "QuotaExceededError");
    const result = node(context, "DelayNode");
    record(result).maximumDelay = maximumDelay;
    param(result, "delayTime", 0, 0, maximumDelay);
    const delayTime = options.delayTime;
    if (delayTime !== undefined) result.delayTime.value = delayTime;
    return result;
  }
  function shaper(context, options = {}) {
    options ??= {};
    const result = node(context, "WaveShaperNode");
    record(result).curve = null;
    record(result).oversample = "none";
    const curve = options.curve,
      oversample = options.oversample;
    if (curve !== undefined) result.curve = curve;
    if (oversample !== undefined) result.oversample = oversample;
    return result;
  }
  function panner(context, options = {}) {
    options ??= {};
    const result = node(context, "StereoPannerNode");
    param(result, "pan", 0, -1, 1);
    const pan = options.pan;
    if (pan !== undefined) result.pan.value = pan;
    return result;
  }
  for (const [kind, factory] of [
    ["BiquadFilterNode", biquad],
    ["WaveShaperNode", shaper],
    ["StereoPannerNode", panner],
  ])
    install(kind, prototypes.AudioNode, 1, function (context, options) {
      required(1, arguments, kind);
      return factory(context, options);
    });
  install("DelayNode", prototypes.AudioNode, 1, function (context, options = {}) {
    required(1, arguments, "DelayNode");
    options ??= {};
    return delay(context, options.maxDelayTime ?? 1, options);
  });
  method("BaseAudioContext", "createDelay", 0, function (maximum = 1) {
    record(this, ["OfflineAudioContext"]);
    return delay(this, maximum);
  });
  getter(
    "BiquadFilterNode",
    "type",
    function () {
      return record(this, ["BiquadFilterNode"]).type;
    },
    function (value) {
      const state = record(this, ["BiquadFilterNode"]);
      value = LocalString(value);
      if (
        includes(
          [
            "lowpass",
            "highpass",
            "bandpass",
            "lowshelf",
            "highshelf",
            "peaking",
            "notch",
            "allpass",
          ],
          value,
        )
      )
        state.type = value;
    },
  );
  method("BiquadFilterNode", "getFrequencyResponse", 3, function (frequencies, magnitude, phase) {
    const state = record(this, ["BiquadFilterNode"]);
    required(3, arguments, "getFrequencyResponse");
    for (const value of [frequencies, magnitude, phase])
      if (apply(typedArrayTag, value, []) !== "Float32Array")
        throw new LocalTypeError("Expected Float32Array");
    const length = apply(typedLength, frequencies, []);
    if (apply(typedLength, magnitude, []) !== length || apply(typedLength, phase, []) !== length)
      throw failure("Frequency response arrays must have equal lengths", "InvalidAccessError");
    if (length > 262144) throw failure("Frequency response limit exceeded", "QuotaExceededError");
    const values = map(
        ["frequency", "Q", "gain", "detune"],
        (name) => record(state.parameters[name]).value,
      ),
      owner = record(state.context);
    const response = ops.op_audio_frequency_response(
      "biquad:" + state.type,
      owner.sampleRate,
      values,
      samples(frequencies, length),
    );
    for (let i = 0; i < length; i++) {
      magnitude[i] = response[0][i];
      phase[i] = response[1][i];
    }
  });
  getter(
    "WaveShaperNode",
    "curve",
    function () {
      return record(this, ["WaveShaperNode"]).curve;
    },
    function (value) {
      const state = record(this, ["WaveShaperNode"]);
      if (value === null) {
        state.curve = null;
        return;
      }
      if (apply(typedArrayTag, value, []) !== "Float32Array")
        throw new LocalTypeError("Expected Float32Array");
      const converted = new FloatArray(value),
        length = apply(typedLength, converted, []);
      if (length < 2)
        throw failure("shaping curve requires at least two values", "InvalidStateError");
      if (length > 262144)
        throw failure("Native shaping curve limit exceeded", "QuotaExceededError");
      state.curve = converted;
    },
  );
  getter(
    "WaveShaperNode",
    "oversample",
    function () {
      return record(this, ["WaveShaperNode"]).oversample;
    },
    function (value) {
      const state = record(this, ["WaveShaperNode"]);
      value = LocalString(value);
      if (!includes(["none", "2x", "4x"], value)) return;
      if (value !== "none") unsupported();
      state.oversample = value;
    },
  );
  for (const key of ["sampleRate", "destination", "state", "currentTime"])
    getter("BaseAudioContext", key, function () {
      const state = record(this, ["OfflineAudioContext"]);
      return key === "currentTime" ? state.time : state[key];
    });
  getter("OfflineAudioContext", "length", function () {
    return record(this, ["OfflineAudioContext"]).length;
  });
  method("BaseAudioContext", "createBuffer", 3, function (channels, length, rate) {
    record(this, ["OfflineAudioContext"]);
    required(3, arguments, "createBuffer");
    return makeBuffer(finite(channels) >>> 0, finite(length) >>> 0, fround(finite(rate)));
  });
  for (const [name, factory] of [
    ["createOscillator", oscillator],
    ["createGain", gain],
    ["createDynamicsCompressor", compressor],
    ["createBufferSource", bufferSource],
    ["createBiquadFilter", biquad],
    ["createWaveShaper", shaper],
    ["createStereoPanner", panner],
  ])
    method("BaseAudioContext", name, 0, function () {
      record(this, ["OfflineAudioContext"]);
      return factory(this);
    });
  for (const key of ["length", "sampleRate", "numberOfChannels", "duration"])
    getter("AudioBuffer", key, function () {
      const state = record(this, ["AudioBuffer"]);
      return key === "numberOfChannels"
        ? state.channels.length
        : key === "duration"
          ? state.length / state.sampleRate
          : state[key];
    });
  method("AudioBuffer", "getChannelData", 1, function (index) {
    const state = record(this, ["AudioBuffer"]);
    required(1, arguments, "getChannelData");
    index = finite(index) >>> 0;
    if (index >= state.channels.length)
      throw failure("channel index exceeds number of channels", "IndexSizeError");
    return state.channels[index];
  });
  for (const to of [false, true])
    method(
      "AudioBuffer",
      to ? "copyToChannel" : "copyFromChannel",
      2,
      function (data, index, offset = 0) {
        const state = record(this, ["AudioBuffer"]);
        required(2, arguments, to ? "copyToChannel" : "copyFromChannel");
        if (apply(typedArrayTag, data, []) !== "Float32Array")
          throw new LocalTypeError("Expected Float32Array");
        index = finite(index) >>> 0;
        offset = finite(offset) >>> 0;
        if (index >= state.channels.length)
          throw failure("channel index exceeds number of channels", "IndexSizeError");
        const channel = state.channels[index];
        const count = maximum(
          0,
          minimum(apply(typedLength, data, []), apply(typedLength, channel, []) - offset),
        );
        if (count === 0) return;
        if (to) apply(typedSet, channel, [apply(typedSubarray, data, [0, count]), offset]);
        else apply(typedSet, data, [apply(typedSubarray, channel, [offset, offset + count])]);
      },
    );
  getter("AudioNode", "context", function () {
    return record(this, nodeKinds).context;
  });
  getter("AudioNode", "numberOfInputs", function () {
    const state = record(this, nodeKinds);
    return includes(["OscillatorNode", "AudioBufferSourceNode"], state.kind) ? 0 : 1;
  });
  getter("AudioNode", "numberOfOutputs", function () {
    return record(this, nodeKinds).kind === "AudioDestinationNode" ? 0 : 1;
  });
  method("AudioNode", "connect", 1, function (destination, output = 0, input = 0) {
    const source = record(this, nodeKinds);
    required(1, arguments, "connect");
    const destinationRecord = record(destination);
    if (destinationRecord.kind === "AudioParam") unsupported();
    const target = record(destination, nodeKinds);
    if (source.context !== target.context)
      throw failure(
        "Failed to execute 'connect' on 'AudioNode': cannot connect to an AudioNode belonging to a different audio context.",
        "InvalidAccessError",
      );
    if (
      finite(output) !== 0 ||
      finite(input) !== 0 ||
      source.kind === "AudioDestinationNode" ||
      includes(["OscillatorNode", "AudioBufferSourceNode"], target.kind)
    )
      throw failure("Audio port index is out of range", "IndexSizeError");
    if (!includes(target.inputs, source.id)) apply(arrayPush, target.inputs, [source.id]);
    return destination;
  });
  method("AudioNode", "disconnect", 0, function (destination) {
    const source = record(this, nodeKinds),
      owner = record(source.context);
    if (destination !== undefined) {
      const target = record(destination, nodeKinds);
      if (target.context !== source.context)
        throw failure("Audio contexts differ", "InvalidAccessError");
      if (!includes(target.inputs, source.id))
        throw failure("No matching connection", "InvalidAccessError");
      target.inputs = filter(target.inputs, (id) => id !== source.id);
    } else
      for (let index = 0; index < owner.nodes.length; index++) {
        const target = record(owner.nodes[index]);
        target.inputs = filter(target.inputs, (id) => id !== source.id);
      }
  });
  method("AudioScheduledSourceNode", "start", 0, function (when = 0) {
    const state = record(this, ["OscillatorNode", "AudioBufferSourceNode"]);
    if (state.started) throw failure("cannot call start more than once", "InvalidStateError");
    when = finite(when);
    if (when < 0) throw new LocalRangeError("start time must be non-negative");
    state.start = when;
    state.started = true;
  });
  method("AudioBufferSourceNode", "start", 0, function (when = 0, offset = 0, duration) {
    const state = record(this, ["AudioBufferSourceNode"]);
    if (state.started) throw failure("cannot call start more than once", "InvalidStateError");
    when = finite(when);
    offset = finite(offset);
    if (duration !== undefined) duration = finite(duration);
    if (when < 0 || offset < 0 || (duration !== undefined && duration < 0))
      throw new LocalRangeError("start arguments must be non-negative");
    state.start = when;
    state.offset = offset;
    state.duration = duration ?? null;
    state.started = true;
  });
  method("AudioScheduledSourceNode", "stop", 0, function (when = 0) {
    const state = record(this, ["OscillatorNode", "AudioBufferSourceNode"]);
    if (!state.started) throw failure("cannot call stop before start", "InvalidStateError");
    when = finite(when);
    if (when < 0) throw new LocalRangeError("stop time must be non-negative");
    state.stopped = when;
  });
  getter(
    "AudioParam",
    "value",
    function () {
      return record(this, ["AudioParam"]).value;
    },
    function (value) {
      const state = record(this, ["AudioParam"]);
      state.value = minimum(state.maxValue, maximum(state.minValue, audioValue(value)));
      schedule(state, {
        kind: "set",
        time: record(record(state.node).context).time,
        value: state.value,
      });
    },
  );
  for (const key of ["defaultValue", "minValue", "maxValue"])
    getter("AudioParam", key, function () {
      return record(this, ["AudioParam"])[key];
    });
  getter(
    "AudioParam",
    "automationRate",
    function () {
      return record(this, ["AudioParam"]).automationRate;
    },
    function (value) {
      const state = record(this, ["AudioParam"]);
      value = LocalString(value);
      if (!includes(["a-rate", "k-rate"], value))
        throw new LocalTypeError("Invalid automationRate");
      if (
        includes(["DynamicsCompressorNode", "AudioBufferSourceNode"], record(state.node).kind) &&
        value !== "k-rate"
      )
        throw failure("DynamicsCompressorNode AudioParams are fixed k-rate", "InvalidStateError");
      state.automationRate = value;
    },
  );
  function audioValue(value) {
    value = fround(finite(value));
    if (!numberFinite(value)) throw new LocalTypeError("The provided float value is non-finite");
    return value;
  }
  function eventTime(value) {
    value = finite(value);
    if (value < 0) throw new LocalRangeError("AudioParam time must be non-negative");
    return value;
  }
  function schedule(state, event) {
    setPrototype(event, null);
    if (state.events.length >= 256)
      throw failure("AudioParam event limit exceeded", "QuotaExceededError");
    for (let i = 0; i < state.events.length; i++) {
      const previous = state.events[i];
      if (
        event.kind !== "curve" &&
        previous.kind === "curve" &&
        event.time >= previous.time &&
        event.time < previous.time + previous.duration
      )
        throw failure("AudioParam event overlaps a value curve", "NotSupportedError");
      if (
        event.kind === "curve" &&
        previous.time >= event.time &&
        previous.time < event.time + event.duration
      )
        throw failure("AudioParam value curve overlaps an event", "NotSupportedError");
    }
    state.events = filter(
      state.events,
      (previous) => previous.time !== event.time || previous.kind !== event.kind,
    );
    apply(arrayPush, state.events, [event]);
    apply(arraySort, state.events, [(a, b) => a.time - b.time]);
  }
  for (const [name, kind] of [
    ["setValueAtTime", "set"],
    ["linearRampToValueAtTime", "linear"],
    ["exponentialRampToValueAtTime", "exponential"],
  ])
    method("AudioParam", name, 2, function (value, time) {
      const state = record(this, ["AudioParam"]);
      required(2, arguments, name);
      value = audioValue(value);
      time = eventTime(time);
      if (kind === "exponential" && value === 0)
        throw new LocalRangeError("exponential ramp target must be non-zero");
      schedule(state, { kind, time, value });
      return this;
    });
  method("AudioParam", "setTargetAtTime", 3, function (value, time, constant) {
    const state = record(this, ["AudioParam"]);
    required(3, arguments, "setTargetAtTime");
    value = audioValue(value);
    time = eventTime(time);
    constant = eventTime(constant);
    schedule(state, { kind: "target", time, value, constant });
    return this;
  });
  method("AudioParam", "setValueCurveAtTime", 3, function (curve, time, duration) {
    const state = record(this, ["AudioParam"]);
    required(3, arguments, "setValueCurveAtTime");
    time = eventTime(time);
    duration = finite(duration);
    if (duration <= 0) throw new LocalRangeError("curve duration must be positive");
    const converted = new FloatArray(curve);
    const count = apply(typedLength, converted, []);
    if (count < 2) throw failure("value curve requires at least two values", "InvalidStateError");
    if (count > 65536) throw failure("AudioParam curve limit exceeded", "QuotaExceededError");
    const values = samples(converted, count);
    for (let i = 0; i < count; i++) finite(values[i]);
    schedule(state, { kind: "curve", time, duration, curve: values });
    schedule(state, { kind: "set", time: time + duration, value: values[count - 1] });
    return this;
  });
  method("AudioParam", "cancelScheduledValues", 1, function (time) {
    const state = record(this, ["AudioParam"]);
    required(1, arguments, "cancelScheduledValues");
    time = eventTime(time);
    state.events = filter(state.events, (event) => event.time < time);
    return this;
  });
  method("AudioParam", "cancelAndHoldAtTime", 1, function (time) {
    const state = record(this, ["AudioParam"]);
    required(1, arguments, "cancelAndHoldAtTime");
    time = eventTime(time);
    const owner = record(record(state.node).context);
    const value = ops.op_audio_param_value(
      state.value,
      { events: state.events },
      time,
      owner.sampleRate,
    );
    let next = null;
    for (let i = 0; i < state.events.length; i++)
      if (state.events[i].time >= time) {
        next = state.events[i];
        break;
      }
    state.events = filter(state.events, (event) => event.time < time);
    if (next && (next.kind === "linear" || next.kind === "exponential"))
      schedule(state, { kind: next.kind, time, value });
    // Internal hold events legitimately terminate an existing curve.
    const held = { kind: "set", time, value };
    setPrototype(held, null);
    apply(arrayPush, state.events, [held]);
    apply(arraySort, state.events, [(a, b) => a.time - b.time]);
    return this;
  });
  for (const [kind, names] of [
    ["OscillatorNode", ["frequency", "detune"]],
    ["GainNode", ["gain"]],
    ["DynamicsCompressorNode", ["threshold", "knee", "ratio", "attack", "release"]],
    ["AudioBufferSourceNode", ["playbackRate", "detune"]],
    ["BiquadFilterNode", ["frequency", "Q", "gain", "detune"]],
    ["DelayNode", ["delayTime"]],
    ["StereoPannerNode", ["pan"]],
  ])
    for (const name of names)
      getter(kind, name, function () {
        return record(this, [kind]).parameters[name];
      });
  getter(
    "OscillatorNode",
    "type",
    function () {
      return record(this, ["OscillatorNode"]).type;
    },
    function (value) {
      const state = record(this, ["OscillatorNode"]);
      value = LocalString(value);
      if (!includes(["sine", "square", "sawtooth", "triangle", "custom"], value)) return;
      if (value === "custom")
        throw failure("Custom waves require a native PeriodicWave", "InvalidStateError");
      state.type = value;
    },
  );
  getter("DynamicsCompressorNode", "reduction", function () {
    return record(this, ["DynamicsCompressorNode"]).reduction;
  });
  method("OscillatorNode", "setPeriodicWave", 1, function () {
    record(this, ["OscillatorNode"]);
    unsupported();
  });
  for (const name of ["buffer", "loop", "loopStart", "loopEnd"])
    getter(
      "AudioBufferSourceNode",
      name,
      function () {
        return record(this, ["AudioBufferSourceNode"])[name];
      },
      function (value) {
        const state = record(this, ["AudioBufferSourceNode"]);
        if (name === "buffer") {
          if (value !== null) record(value, ["AudioBuffer"]);
          if (value !== null && state.bufferSet)
            throw failure("buffer may only be set once", "InvalidStateError");
          state.buffer = value;
          if (value !== null) state.bufferSet = true;
        } else state[name] = name === "loop" ? LocalBoolean(value) : finite(value);
      },
    );
  method("OfflineAudioContext", "startRendering", 0, function () {
    const context = this,
      state = record(context, ["OfflineAudioContext"]);
    if (state.rendering)
      return rejected(
        failure(
          "Failed to execute 'startRendering' on 'OfflineAudioContext': cannot call startRendering on an OfflineAudioContext in a stopped state.",
          "InvalidStateError",
        ),
      );
    state.rendering = true;
    state.state = "running";
    const rendering = apply(promiseThen, apply(promiseResolve, LocalPromise, []), [
      () => {
        dispatchHost(new LocalEvent("statechange"), context);
        const nodes = map(state.nodes, (entry) => {
          const source = record(entry);
          const values = source.parameters;
          const result = {
            kind: {
              AudioDestinationNode: "destination",
              OscillatorNode: "oscillator",
              GainNode: "gain",
              DynamicsCompressorNode: "compressor",
              AudioBufferSourceNode: "buffer",
              BiquadFilterNode: "biquad:" + source.type,
              DelayNode: "delay",
              WaveShaperNode: "waveshaper",
              StereoPannerNode: "panner",
            }[source.kind],
            inputs: source.inputs,
            started: source.started,
            stop: source.stopped,
            start: source.start ?? 0,
            offset: source.offset ?? 0,
            duration: source.duration ?? null,
          };
          if (source.kind === "OscillatorNode") {
            result.shape = source.type;
            result.parameters = [record(values.frequency).value, record(values.detune).value];
          }
          if (source.kind === "GainNode") result.parameters = [record(values.gain).value];
          if (source.kind === "DynamicsCompressorNode")
            result.parameters = map(
              ["threshold", "knee", "ratio", "attack", "release"],
              (name) => record(values[name]).value,
            );
          if (source.kind === "AudioBufferSourceNode") {
            result.parameters = [record(values.playbackRate).value, record(values.detune).value];
            result.looped = source.loop;
            result.loopStart = source.loopStart;
            result.loopEnd = source.loopEnd;
            if (source.buffer !== null) {
              const input = record(source.buffer, ["AudioBuffer"]);
              result.bufferRate = input.sampleRate;
              result.data = map(input.channels, (channel) => samples(channel, input.length));
            }
          }
          if (source.kind === "BiquadFilterNode")
            result.parameters = map(
              ["frequency", "Q", "gain", "detune"],
              (name) => record(values[name]).value,
            );
          if (source.kind === "DelayNode")
            result.parameters = [record(values.delayTime).value, source.maximumDelay];
          if (source.kind === "StereoPannerNode") result.parameters = [record(values.pan).value];
          if (source.kind === "WaveShaperNode")
            result.curve =
              source.curve === null
                ? []
                : samples(source.curve, apply(typedLength, source.curve, []));
          const names =
            {
              OscillatorNode: ["frequency", "detune"],
              GainNode: ["gain"],
              DynamicsCompressorNode: ["threshold", "knee", "ratio", "attack", "release"],
              AudioBufferSourceNode: ["playbackRate", "detune"],
              BiquadFilterNode: ["frequency", "Q", "gain", "detune"],
              DelayNode: ["delayTime"],
              StereoPannerNode: ["pan"],
            }[source.kind] ?? [];
          result.automation = map(names, (name) => {
            const parameter = record(values[name]);
            return {
              events: parameter.events,
              kRate: parameter.automationRate === "k-rate",
              inputs: parameter.inputs,
            };
          });
          return result;
        });
        let rendered;
        try {
          rendered = ops.op_audio_render({
            channels: state.channels,
            length: state.length,
            sampleRate: state.sampleRate,
            nodes,
          });
        } catch (error) {
          state.state = "closed";
          throw failure(
            error.message,
            error.message.includes("limit") ? "QuotaExceededError" : "NotSupportedError",
          );
        }
        const buffer = makeBuffer(
          state.channels,
          state.length,
          state.sampleRate,
          rendered.channels,
        );
        // Completion follows the measured offline lifecycle: ended events run
        // while the context is running, then complete, then closed statechange.
        for (let index = 0; index < state.nodes.length; index++) {
          const entry = state.nodes[index];
          const source = record(entry);
          if (source.kind === "DynamicsCompressorNode")
            source.reduction = rendered.reductions[index] ?? 0;
          const finalValues = rendered.finalValues[index];
          const names =
            {
              OscillatorNode: ["frequency", "detune"],
              GainNode: ["gain"],
              DynamicsCompressorNode: ["threshold", "knee", "ratio", "attack", "release"],
              AudioBufferSourceNode: ["playbackRate", "detune"],
              BiquadFilterNode: ["frequency", "Q", "gain", "detune"],
              DelayNode: ["delayTime"],
              StereoPannerNode: ["pan"],
            }[source.kind] ?? [];
          if (finalValues)
            for (let parameter = 0; parameter < names.length; parameter++)
              record(source.parameters[names[parameter]]).value = finalValues[parameter];
          if (!source.started) continue;
          const end =
            rendered.ends[index] === undefined ? null : rendered.ends[index] / state.sampleRate;
          if (end !== null && end <= rendered.currentTime) {
            state.time = minimum(
              rendered.currentTime,
              (ceiling((end * state.sampleRate) / 128) * 128) / state.sampleRate,
            );
            dispatchHost(new LocalEvent("ended"), entry);
          }
        }
        state.time = rendered.currentTime;
        state.state = "closed";
        const event = new constructors.OfflineAudioCompletionEvent("complete", {
          renderedBuffer: buffer,
        });
        dispatchHost(event, context);
        dispatchHost(new LocalEvent("statechange"), context);
        return buffer;
      },
    ]);
    return apply(promiseThen, rendering, [
      undefined,
      (error) => {
        state.state = "closed";
        throw error;
      },
    ]);
  });
  for (const name of ["suspend", "resume"])
    method("OfflineAudioContext", name, 0, function () {
      record(this, ["OfflineAudioContext"]);
      return rejected(failure("Offline render suspension is not supported"));
    });
  for (const kind of ["BaseAudioContext", "AudioScheduledSourceNode"]) {
    const eventName = kind === "BaseAudioContext" ? "statechange" : "ended";
    const kinds =
      kind === "BaseAudioContext"
        ? ["OfflineAudioContext"]
        : ["OscillatorNode", "AudioBufferSourceNode"];
    getter(
      kind,
      "on" + eventName,
      function () {
        return record(this, kinds).handlers?.[eventName] ?? null;
      },
      function (handler) {
        const state = record(this, kinds);
        state.handlers ||= create(null);
        if (state.handlers[eventName])
          apply(eventRemove, this, [eventName, state.handlers[eventName]]);
        state.handlers[eventName] = typeof handler === "function" ? handler : null;
        if (state.handlers[eventName])
          apply(eventAdd, this, [eventName, state.handlers[eventName]]);
      },
    );
  }
  getter(
    "OfflineAudioContext",
    "oncomplete",
    function () {
      return record(this, ["OfflineAudioContext"]).complete ?? null;
    },
    function (handler) {
      const state = record(this, ["OfflineAudioContext"]);
      if (state.complete) apply(eventRemove, this, ["complete", state.complete]);
      state.complete = typeof handler === "function" ? handler : null;
      if (state.complete) apply(eventAdd, this, ["complete", state.complete]);
    },
  );
});
