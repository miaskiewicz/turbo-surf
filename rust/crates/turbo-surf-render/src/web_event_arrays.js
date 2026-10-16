// Private dense-array operations for the pinned official Deno event algorithms.
// These arrays never cross the native wrapper boundary; public Array is untouched.
(primordials) => {
  const define = primordials.ObjectDefineProperty,
    apply = primordials.ReflectApply;
  const own = (array, index, value) =>
    define(array, index, {
      __proto__: null,
      value,
      enumerable: true,
      writable: true,
      configurable: true,
    });
  const push = (array, ...items) => {
    let length = array.length;
    for (let index = 0; index < items.length; index++) own(array, length++, items[index]);
    return length;
  };
  const slice = (array) => {
    const result = [];
    for (let index = 0; index < array.length; index++) own(result, index, array[index]);
    return result;
  };
  const map = (array, callback) => {
    const result = [];
    for (let index = 0; index < array.length; index++)
      own(result, index, apply(callback, undefined, [array[index], index, array]));
    return result;
  };
  const splice = (array, index, count) => {
    const result = [],
      length = array.length;
    for (let offset = 0; offset < count; offset++) own(result, offset, array[index + offset]);
    for (let at = index; at + count < length; at++) own(array, at, array[at + count]);
    array.length = length - count;
    return result;
  };
  const unshift = (array, ...items) => {
    const count = items.length,
      length = array.length;
    for (let index = length; index > 0; index--) own(array, index - 1 + count, array[index - 1]);
    for (let index = 0; index < count; index++) own(array, index, items[index]);
    return length + count;
  };
  return {
    __proto__: null,
    ArrayPrototypePush: push,
    ArrayPrototypeSlice: slice,
    ArrayPrototypeMap: map,
    ArrayPrototypeSplice: splice,
    ArrayPrototypeUnshift: unshift,
  };
};
