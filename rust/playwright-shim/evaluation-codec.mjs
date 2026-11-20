// A toJSON-free serializer for the explicit-tag tuple shape below (#531, split
// from #527). `JSON.stringify` invokes `value.toJSON()` by spec, so a guest that
// sets `Object.prototype.toJSON` or `Array.prototype.toJSON` can corrupt the
// result envelope the host relies on. The encoded tuple is finite: strings,
// finite numbers, booleans, null, nested arrays of the same. This hand-written
// encoder covers that shape without calling any guest-reachable method — valid
// only for the output of `encodeEvaluationValue`, not general values.
export function safeStringifyTuple(value) {
  if (value === null) return "null";
  if (value === true) return "true";
  if (value === false) return "false";
  const kind = typeof value;
  if (kind === "number") return Number.isFinite(value) ? String(value) : "null";
  if (kind === "string") {
    let out = '"';
    for (let i = 0; i < value.length; i++) {
      const c = value.charCodeAt(i);
      if (c === 0x22) out += '\\"';
      else if (c === 0x5c) out += "\\\\";
      else if (c === 0x08) out += "\\b";
      else if (c === 0x09) out += "\\t";
      else if (c === 0x0a) out += "\\n";
      else if (c === 0x0c) out += "\\f";
      else if (c === 0x0d) out += "\\r";
      else if (c < 0x20 || c === 0x7f) out += `\\u${c.toString(16).padStart(4, "0")}`;
      else out += value[i];
    }
    return out + '"';
  }
  if (Array.isArray(value)) {
    let out = "[";
    for (let i = 0; i < value.length; i++) {
      if (i > 0) out += ",";
      out += safeStringifyTuple(value[i]);
    }
    return out + "]";
  }
  throw new TypeError("safeStringifyTuple: unencodable value");
}

// The bridge carries strings; explicit tags preserve values JSON would erase.
export function encodeEvaluationValue(value, references = new Map()) {
  if (value === undefined || typeof value === "function" || typeof value === "symbol")
    return ["undefined"];
  if (typeof value === "bigint") return ["bigint", String(value)];
  if (typeof value === "number") {
    if (Object.is(value, -0)) return ["number", "-0"];
    if (!Number.isFinite(value)) return ["number", String(value)];
  }
  if (value === null || typeof value !== "object") return ["value", value];
  const brand = Object.prototype.toString.call(value);
  if (brand === "[object Date]") return ["date", value.toISOString()];
  if (brand === "[object RegExp]") return ["regexp", value.source, value.flags];
  if (references.has(value)) return ["reference", references.get(value)];
  const id = references.size;
  references.set(value, id);
  if (Array.isArray(value))
    return ["array", id, Array.from(value, (entry) => encodeEvaluationValue(entry, references))];
  return [
    "object",
    id,
    Object.entries(value).map(([key, entry]) => [key, encodeEvaluationValue(entry, references)]),
  ];
}

export function decodeEvaluationValue(value, references = new Map()) {
  const [kind, payload, entries] = value;
  if (kind === "undefined") return undefined;
  if (kind === "value") return payload;
  if (kind === "number") return payload === "-0" ? -0 : Number(payload);
  if (kind === "bigint") return BigInt(payload);
  if (kind === "date") return new Date(payload);
  if (kind === "regexp") return new RegExp(payload, entries);
  if (kind === "reference") return references.get(payload);
  const result = kind === "array" ? [] : {};
  references.set(payload, result);
  if (kind === "array") {
    for (const entry of entries) result.push(decodeEvaluationValue(entry, references));
  } else {
    for (const [key, entry] of entries)
      Object.defineProperty(result, key, {
        value: decodeEvaluationValue(entry, references),
        enumerable: true,
        writable: true,
        configurable: true,
      });
  }
  return result;
}

export function evaluationArgument(value) {
  const encoded = encodeEvaluationValue(value);
  if (encoded[0] === "undefined") return "undefined";
  if (encoded[0] === "value") return JSON.stringify(encoded[1]);
  return `(${decodeEvaluationValue.toString()})(${JSON.stringify(encoded)})`;
}

export function encodeEvaluationError(error) {
  return {
    name: String(error?.name ?? "Error"),
    message: String(error?.message ?? error),
    stack: typeof error?.stack === "string" ? error.stack : undefined,
  };
}

// Serialize the `{value: ...}` / `{error: ...}` envelope the guest sends back
// to the host without invoking `JSON.stringify` — guest `Object.prototype.toJSON`
// or `Array.prototype.toJSON` hooks would corrupt the wire otherwise. The value
// payload is the tuple output of `encodeEvaluationValue`; the error payload is
// the plain-string object from `encodeEvaluationError`.
export function safeStringifyEnvelope(key, payload) {
  if (key === "value") return '{"value":' + safeStringifyTuple(payload) + "}";
  if (key === "error") {
    const err = payload;
    let out = '{"error":{"name":' + safeStringifyTuple(err.name);
    out += ',"message":' + safeStringifyTuple(err.message);
    if (err.stack !== undefined) out += ',"stack":' + safeStringifyTuple(err.stack);
    return out + "}}";
  }
  throw new TypeError("safeStringifyEnvelope: unknown key " + key);
}

export function decodeEvaluationError(value) {
  const constructors = {
    Error,
    TypeError,
    RangeError,
    ReferenceError,
    SyntaxError,
    EvalError,
    URIError,
  };
  const Constructor = Object.hasOwn(constructors, value.name) ? constructors[value.name] : Error;
  const error = new Constructor(value.message);
  error.name = value.name;
  if (value.stack) error.stack = value.stack;
  return error;
}
