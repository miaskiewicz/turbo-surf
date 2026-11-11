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
