import { isDeepStrictEqual } from "node:util";

const tagOf = (value) => Object.prototype.toString.call(value);
const scalarTags = new Set([
  "[object Date]",
  "[object Number]",
  "[object String]",
  "[object Boolean]",
  "[object BigInt]",
  "[object Symbol]",
]);
const opaqueTags = new Set(["[object WeakMap]", "[object WeakSet]", "[object Promise]"]);

function keysOf(value, strict) {
  return Reflect.ownKeys(value).filter(
    (key) =>
      Object.prototype.propertyIsEnumerable.call(value, key) &&
      (strict || value[key] !== undefined),
  );
}

function unorderedEqual(left, right, matches) {
  if (left.length !== right.length) return false;
  const remaining = [...right];
  for (const value of left) {
    const index = remaining.findIndex((other) => matches(value, other));
    if (index < 0) return false;
    remaining.splice(index, 1);
  }
  return true;
}

function objectEqual(a, b, strict, stack) {
  const left = keysOf(a, strict);
  const right = keysOf(b, strict);
  return (
    left.length === right.length &&
    left.every((key) => right.includes(key) && compare(a[key], b[key], strict, stack))
  );
}

function contentsEqual(a, b, strict, stack, tag) {
  const equal = (left, right) => compare(left, right, strict, stack);
  if (scalarTags.has(tag)) return Object.is(a.valueOf(), b.valueOf());
  if (tag === "[object RegExp]") return a.source === b.source && a.flags === b.flags;
  if (tag === "[object Error]") return a.message === b.message;
  if (opaqueTags.has(tag)) return false;
  if (tag === "[object Map]")
    return unorderedEqual(
      [...a],
      [...b],
      (left, right) => equal(left[0], right[0]) && equal(left[1], right[1]),
    );
  if (tag === "[object Set]") return unorderedEqual([...a], [...b], equal);
  if (Array.isArray(a) && strict && a.length !== b.length) return false;
  if (tag !== "[object Object]" && !Array.isArray(a)) return isDeepStrictEqual(a, b);
  return objectEqual(a, b, strict, stack);
}

function compare(a, b, strict, stack) {
  if (typeof b?.asymmetricMatch === "function") return b.asymmetricMatch(a);
  if (typeof a?.asymmetricMatch === "function") return a.asymmetricMatch(b);
  if (Object.is(a, b)) return true;
  if (a == null || b == null || typeof a !== "object" || typeof b !== "object") return false;
  const tag = tagOf(a);
  if (tag !== tagOf(b)) return false;
  if (strict && Object.getPrototypeOf(a) !== Object.getPrototypeOf(b)) return false;
  const previous = stack.find(([left, right]) => left === a || right === b);
  if (previous) return previous[0] === a && previous[1] === b;
  stack.push([a, b]);
  try {
    return contentsEqual(a, b, strict, stack, tag);
  } finally {
    stack.pop();
  }
}

export function equalValues(a, b, strict = false) {
  return compare(a, b, strict, []);
}
