// Geometry interfaces operate on private column-major matrices. The browser
// environment binds these delegates with native FunctionTemplates afterward.
// The pure helpers stay inside this closure so no name leaks into the shared
// bootstrap scope this file is concatenated into.
//
// Chrome's members never consult page-replaceable intrinsics or one another
// through the prototype (turbo-surf#70): every builtin used after bootstrap is
// captured here, internal arrays are created as literals (never through a
// page-patchable iterator, setter or Array static) and members share private
// value helpers instead of calling each other's public methods.
// oxlint-disable unicorn/consistent-function-scoping
(() => {
  const matrices = new WeakMap(),
    // Blink's `is2d_` (turbo-surf#89): whether a matrix is 2D is state set by its
    // input and operations, not derived from its values.
    flatMatrices = new WeakMap(),
    points = new WeakMap(),
    // DOMPoint/DOMMatrix platform objects (not their read-only bases), recorded by the
    // interface constructor that ran, as Chrome serializes the wrapper's interface.
    writable = new WeakSet();
  const apply = Reflect.apply,
    create = Object.create,
    construct = Reflect.construct,
    same = Object.is,
    finite = Number.isFinite,
    arrayFrom = Array.from,
    weakGet = WeakMap.prototype.get,
    weakSet = WeakMap.prototype.set,
    isWritable = WeakSet.prototype.has,
    markWritable = WeakSet.prototype.add,
    Text = String,
    Float32 = Float32Array,
    Float64 = Float64Array,
    IdlTypeError = TypeError,
    IdlDOMException = DOMException,
    { abs, atan2, cos, fround, hypot, max, min, sin, tan, PI, SQRT1_2 } = Math;
  // WebIDL `unrestricted double`: ToNumber, which (unlike `Number()`) rejects BigInt.
  const toNumber = (value) => +value;
  const read = (map, receiver) => apply(weakGet, map, [receiver]);
  const store = (map, receiver, values) => {
    apply(weakSet, map, [receiver, values]);
  };
  const identity = () => [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];
  const matrix = (receiver) => {
    const data = read(matrices, receiver);
    if (!data) throw new IdlTypeError("Illegal invocation");
    return data;
  };
  // A matrix's state: [16 column-major values, is2D flag].
  const state = (receiver) => [matrix(receiver), read(flatMatrices, receiver) === true];
  const assign = (receiver, values, flat) => {
    store(matrices, receiver, values);
    store(flatMatrices, receiver, flat);
  };
  const point = (receiver) => {
    const data = read(points, receiver);
    if (!data) throw new IdlTypeError("Illegal invocation");
    return data;
  };
  // Platform objects built without running (page-replaceable) public constructors.
  const newPoint = (x, y, z, w) => {
    const result = create(DOMPoint.prototype);
    store(points, result, [x, y, z, w]);
    apply(markWritable, writable, [result]);
    return result;
  };
  const newMatrix = (values, flat, mutable = true) => {
    const result = create(mutable ? DOMMatrix.prototype : DOMMatrixReadOnly.prototype);
    assign(result, values, flat);
    if (mutable) apply(markWritable, writable, [result]);
    return result;
  };
  const multiply = (a, b) => {
    const result = identity();
    for (let index = 0; index < 16; index++) {
      const column = index >> 2,
        row = index % 4;
      result[index] =
        a[row] * b[column * 4] +
        a[4 + row] * b[column * 4 + 1] +
        a[8 + row] * b[column * 4 + 2] +
        a[12 + row] * b[column * 4 + 3];
    }
    return result;
  };
  const is2D = (value) =>
    value[2] === 0 &&
    value[3] === 0 &&
    value[6] === 0 &&
    value[7] === 0 &&
    value[8] === 0 &&
    value[9] === 0 &&
    value[11] === 0 &&
    value[14] === 0 &&
    value[10] === 1 &&
    value[15] === 1;
  const isIdentity = (value) => {
    const unit = identity();
    for (let index = 0; index < 16; index++) if (value[index] !== unit[index]) return false;
    return true;
  };
  // A 6-value (2D) or 16-value internal array as 16 column-major entries.
  const expand = (values) => {
    if (values.length === 16) return values;
    if (values.length !== 6) throw new IdlTypeError("A matrix requires 6 or 16 values");
    return [
      values[0],
      values[1],
      0,
      0,
      values[2],
      values[3],
      0,
      0,
      0,
      0,
      1,
      0,
      values[4],
      values[5],
      0,
      1,
    ];
  };
  // WebIDL sequence<unrestricted double>: the page's iterable protocol applies.
  // [values, is2D]: 6 values make a 2D matrix, 16 a 3D one whatever they are.
  const fromSequence = (input) => {
    const values = arrayFrom(input, toNumber);
    return [expand(values), values.length === 6];
  };
  // The CSS transform parser (turbo-surf#82) scans characters itself: String and
  // RegExp methods are page-replaceable, Chrome's parser is native.
  const space = (ch) =>
    ch === " " ||
    ch === "\t" ||
    ch === "\n" ||
    ch === "\r" ||
    ch === "\f" ||
    ch === "\v" ||
    ch === "\u00a0" ||
    ch === "\u1680" ||
    (ch >= "\u2000" && ch <= "\u200a") ||
    ch === "\u2028" ||
    ch === "\u2029" ||
    ch === "\u202f" ||
    ch === "\u205f" ||
    ch === "\u3000" ||
    ch === "\ufeff";
  const digit = (ch) => ch >= "0" && ch <= "9";
  const lowerTable = { __proto__: null };
  for (const letter of "ABCDEFGHIJKLMNOPQRSTUVWXYZ") lowerTable[letter] = letter.toLowerCase();
  const lower = (text) => {
    let result = "";
    for (let at = 0; at < text.length; at++) result += lowerTable[text[at]] ?? text[at];
    return result;
  };
  const substring = (text, start, end) => {
    let result = "";
    for (let at = start; at < end; at++) result += text[at];
    return result;
  };
  // `[+-]?(\d+(\.\d*)?|\.\d+)(e[+-]?\d+)?` followed by an optional unit:
  // [number, lower-cased unit], or null.
  const dimension = (input) => {
    let at = 0;
    if (input[at] === "+" || input[at] === "-") at++;
    const integer = at;
    while (at < input.length && digit(input[at])) at++;
    let digits = at > integer;
    if (input[at] === ".") {
      at++;
      const fraction = at;
      while (at < input.length && digit(input[at])) at++;
      digits = digits || at > fraction;
    }
    if (!digits) return null;
    if (input[at] === "e" || input[at] === "E") {
      let exponent = at + 1;
      if (input[exponent] === "+" || input[exponent] === "-") exponent++;
      const start = exponent;
      while (exponent < input.length && digit(input[exponent])) exponent++;
      if (exponent > start) at = exponent;
    }
    return [toNumber(substring(input, 0, at)), lower(substring(input, at, input.length))];
  };
  // CSS angles in degrees, the unit gfx::Transform rotates by.
  const angleUnits = { __proto__: null, deg: 1, rad: 180 / PI, grad: 0.9, turn: 360 };
  const angle = (input) => {
    const parsed = dimension(input);
    if (!parsed || (parsed[1] === "" ? parsed[0] !== 0 : !angleUnits[parsed[1]]))
      throw new IdlDOMException("Invalid transform angle", "SyntaxError");
    return parsed[0] * (angleUnits[parsed[1]] || 1);
  };
  const length = (input) => {
    const parsed = dimension(input);
    if (!parsed || (parsed[1] === "" ? parsed[0] !== 0 : parsed[1] !== "px"))
      throw new IdlDOMException("Invalid absolute transform length", "SyntaxError");
    return parsed[0];
  };
  // gfx::SinCosDegrees (turbo-surf#81): multiples of 90 degrees are exact, and
  // multiples of 45 degrees use the correctly rounded sqrt(1/2).
  const quarterTurns = [
      [0, 1],
      [1, 0],
      [0, -1],
      [-1, 0],
    ],
    eighthTurns = [
      [SQRT1_2, SQRT1_2],
      [SQRT1_2, -SQRT1_2],
      [-SQRT1_2, -SQRT1_2],
      [-SQRT1_2, SQRT1_2],
    ];
  const sinCosDegrees = (degrees) => {
    if (degrees % 90 === 0) return quarterTurns[(((degrees / 90) % 4) + 4) % 4];
    if (degrees % 45 === 0) return eighthTurns[(((((degrees - 45) / 90) % 4) + 4) % 4)];
    const radians = (degrees * PI) / 180;
    return [sin(radians), cos(radians)];
  };
  const rotation = (x, y, z, degrees) => {
    const magnitude = hypot(x, y, z);
    if (!magnitude) return identity();
    x /= magnitude;
    y /= magnitude;
    z /= magnitude;
    const turn = sinCosDegrees(degrees),
      s = turn[0],
      c = turn[1],
      t = 1 - c;
    // gfx::Transform rotates about a principal axis without the general
    // formula's 1 - cos rounding (e.g. rotate(135deg) keeps m33 exactly 1).
    if (x === 0 && y === 0) {
      const sz = z < 0 ? -s : s;
      return [c, sz, 0, 0, -sz, c, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];
    }
    if (y === 0 && z === 0) {
      const sx = x < 0 ? -s : s;
      return [1, 0, 0, 0, 0, c, sx, 0, 0, -sx, c, 0, 0, 0, 0, 1];
    }
    if (x === 0 && z === 0) {
      const sy = y < 0 ? -s : s;
      return [c, 0, -sy, 0, 0, 1, 0, 0, sy, 0, c, 0, 0, 0, 0, 1];
    }
    return [
      t * x * x + c,
      t * x * y + s * z,
      t * x * z - s * y,
      0,
      t * x * y - s * z,
      t * y * y + c,
      t * y * z + s * x,
      0,
      t * x * z + s * y,
      t * y * z - s * x,
      t * z * z + c,
      0,
      0,
      0,
      0,
      1,
    ];
  };
  // Value transforms shared by the copying and the *Self members.
  const translated = (values, tx, ty, tz) => {
    const value = identity();
    value[12] = toNumber(tx);
    value[13] = toNumber(ty);
    value[14] = toNumber(tz);
    return multiply(values, value);
  };
  const scaled = (values, scaleX, scaleY, scaleZ, originX, originY, originZ) => {
    const value = identity();
    value[0] = toNumber(scaleX);
    value[5] = toNumber(scaleY);
    value[10] = toNumber(scaleZ);
    const x = toNumber(originX),
      y = toNumber(originY),
      z = toNumber(originZ);
    return translated(multiply(translated(values, x, y, z), value), -x, -y, -z);
  };
  const rotatedAxisAngle = (values, x, y, z, degrees) =>
    multiply(
      values,
      rotation(toNumber(x), toNumber(y), toNumber(z), toNumber(degrees)),
    );
  const rotated = (values, rotX, rotY, rotZ) => {
    if (rotY === undefined && rotZ === undefined) {
      rotZ = rotX;
      rotX = 0;
      rotY = 0;
    }
    const z = toNumber(rotZ ?? 0),
      y = toNumber(rotY ?? 0),
      x = toNumber(rotX);
    const result = rotatedAxisAngle(
      rotatedAxisAngle(rotatedAxisAngle(values, 0, 0, 1, z), 0, 1, 0, y),
      1,
      0,
      0,
      x,
    );
    // Blink: a rotation about the x or y axis makes the matrix 3D.
    return [result, x === 0 && y === 0];
  };
  const skewed = (values, index, degrees) => {
    const value = identity();
    value[index] = tan((toNumber(degrees) * PI) / 180);
    return multiply(values, value);
  };
  const transformed = (value, p) => {
    const row = (index) =>
      value[index] * p[0] +
      value[4 + index] * p[1] +
      value[8 + index] * p[2] +
      value[12 + index] * p[3];
    return newPoint(row(0), row(1), row(2), row(3));
  };
  const transformArguments = (args) => {
    const values = [];
    for (let index = 0; index < args.length; index++) {
      const value = args[index];
      if (!finite(value)) throw new IdlDOMException("Invalid transform number", "SyntaxError");
      values[index] = value;
    }
    return values;
  };
  const numbers = (args) => {
    const values = [];
    for (let index = 0; index < args.length; index++) values[index] = toNumber(args[index]);
    return values;
  };
  const translations = {
      __proto__: null,
      translate: 1,
      translatex: 1,
      translatey: 1,
      translatez: 1,
      translate3d: 1,
    },
    scalings = { __proto__: null, scale: 1, scalex: 1, scaley: 1, scalez: 1, scale3d: 1 },
    rotations = { __proto__: null, rotate: 1, rotatex: 1, rotatey: 1, rotatez: 1 };
  const transformFunction = (name, args) => {
    const value = identity();
    if (name === "matrix" || name === "matrix3d") {
      if (args.length !== (name === "matrix" ? 6 : 16))
        throw new IdlDOMException("Invalid matrix", "SyntaxError");
      return expand(transformArguments(numbers(args)));
    }
    if (translations[name]) {
      const counts =
        name === "translate"
          ? args.length >= 1 && args.length <= 2
          : name === "translate3d"
            ? args.length === 3
            : args.length === 1;
      if (!counts) throw new IdlDOMException("Invalid translation", "SyntaxError");
      const amounts = [];
      for (let index = 0; index < args.length; index++) amounts[index] = length(args[index]);
      if (name === "translatex") value[12] = amounts[0];
      else if (name === "translatey") value[13] = amounts[0];
      else if (name === "translatez") value[14] = amounts[0];
      else {
        value[12] = amounts[0];
        value[13] = amounts[1] || 0;
        value[14] = amounts[2] || 0;
      }
      return value;
    }
    if (scalings[name]) {
      if (
        !(name === "scale"
          ? args.length >= 1 && args.length <= 2
          : name === "scale3d"
            ? args.length === 3
            : args.length === 1)
      )
        throw new IdlDOMException("Invalid scaling", "SyntaxError");
      const factors = transformArguments(numbers(args));
      if (name === "scalex") value[0] = factors[0];
      else if (name === "scaley") value[5] = factors[0];
      else if (name === "scalez") value[10] = factors[0];
      else {
        value[0] = factors[0];
        value[5] = factors[1] ?? factors[0];
        value[10] = factors[2] ?? 1;
      }
      return value;
    }
    if (rotations[name]) {
      if (args.length !== 1) throw new IdlDOMException("Invalid rotation", "SyntaxError");
      return rotation(
        name === "rotatex" ? 1 : 0,
        name === "rotatey" ? 1 : 0,
        name === "rotatex" || name === "rotatey" ? 0 : 1,
        angle(args[0]),
      );
    }
    if (name === "rotate3d") {
      if (args.length !== 4) throw new IdlDOMException("Invalid rotation", "SyntaxError");
      const xyz = numbers([args[0], args[1], args[2]]);
      if (!finite(xyz[0]) || !finite(xyz[1]) || !finite(xyz[2]))
        throw new IdlDOMException("Invalid rotation", "SyntaxError");
      return rotation(xyz[0], xyz[1], xyz[2], angle(args[3]));
    }
    if (name === "skew" || name === "skewx" || name === "skewy") {
      if (!(name === "skew" ? args.length >= 1 && args.length <= 2 : args.length === 1))
        throw new IdlDOMException("Invalid skew", "SyntaxError");
      const skew = (input) => tan((angle(input) * PI) / 180);
      if (name !== "skewy") value[4] = skew(args[0]);
      if (name === "skewy") value[1] = skew(args[0]);
      else if (args[1]) value[1] = skew(args[1]);
      return value;
    }
    if (name === "perspective" && args.length === 1) {
      const distance = length(args[0]);
      if (distance <= 0) throw new IdlDOMException("Invalid perspective", "SyntaxError");
      value[11] = -1 / distance;
      return value;
    }
    throw new IdlDOMException("Unsupported CSS transform", "SyntaxError");
  };
  // Blink's TransformOperation::Is3DOperation is by function type: any of these makes
  // the parsed matrix 3D, even when its values are 2D-shaped.
  const threeDFunctions = {
    __proto__: null,
    matrix3d: 1,
    translate3d: 1,
    translatez: 1,
    scale3d: 1,
    scalez: 1,
    rotatex: 1,
    rotatey: 1,
    rotate3d: 1,
    perspective: 1,
  };
  // `name(args)` functions separated by whitespace; arguments split on commas and
  // whitespace runs, empty ones dropped. Returns [values, is2D].
  const parse = (source) => {
    let start = 0,
      end = source.length;
    while (start < end && space(source[start])) start++;
    while (end > start && space(source[end - 1])) end--;
    if (start === end || substring(source, start, end) === "none") return [identity(), true];
    let result = identity(),
      flat = true,
      at = start;
    const invalid = () => new IdlDOMException("Invalid CSS transform", "SyntaxError");
    while (at < end) {
      const nameStart = at;
      while (
        at < end &&
        (digit(source[at]) || lowerTable[source[at]] || (source[at] >= "a" && source[at] <= "z"))
      )
        at++;
      if (at === nameStart || source[at] !== "(") throw invalid();
      const name = lower(substring(source, nameStart, at)),
        args = [];
      at++;
      let token = "";
      while (at < end && source[at] !== ")") {
        const ch = source[at];
        if (ch === "(") throw invalid();
        if (ch === "," || space(ch)) {
          if (token) args[args.length] = token;
          token = "";
        } else token += ch;
        at++;
      }
      if (source[at] !== ")") throw invalid();
      if (token) args[args.length] = token;
      at++;
      result = multiply(result, transformFunction(name, args));
      if (threeDFunctions[name]) flat = false;
      while (at < end && space(source[at])) at++;
    }
    return [result, flat];
  };
  const isObject = (value) =>
    value !== null && (typeof value === "object" || typeof value === "function");
  // WebIDL dictionaries (turbo-surf#69/#79): each member is read, then converted,
  // in lexicographic order; conversion errors carry Blink's context.
  const member = (source, name, fallback, prefix, type) => {
    const value = source[name];
    if (value === undefined) return fallback;
    const context = `${prefix}Failed to read the '${name}' property from '${type}': `;
    if (typeof value === "bigint")
      throw new IdlTypeError(`${context}Cannot convert a BigInt value to a number`);
    if (typeof value === "symbol")
      throw new IdlTypeError(`${context}Cannot convert a Symbol value to a number`);
    return toNumber(value);
  };
  const pointInit = (value, prefix) => {
    if (value === undefined || value === null) return [0, 0, 0, 1];
    if (!isObject(value))
      throw new IdlTypeError(`${prefix}The provided value is not of type 'DOMPointInit'.`);
    const w = member(value, "w", 1, prefix, "DOMPointInit"),
      x = member(value, "x", 0, prefix, "DOMPointInit"),
      y = member(value, "y", 0, prefix, "DOMPointInit"),
      z = member(value, "z", 0, prefix, "DOMPointInit");
    return [x, y, z, w];
  };
  const matrixKeys = [
    ["a", 0],
    ["b", 1],
    ["c", 4],
    ["d", 5],
    ["e", 12],
    ["f", 13],
  ];
  // DOMMatrixInit inherits DOMMatrix2DInit: the base dictionary's members are
  // converted first, each group in lexicographic order.
  const matrix2DMembers = ["a", "b", "c", "d", "e", "f", "m11", "m12", "m21", "m22", "m41", "m42"],
    matrix3DMembers = ["m13", "m14", "m23", "m24", "m31", "m32", "m33", "m34", "m43", "m44"];
  const matrixIndex = (key) => (key.charCodeAt(1) - 49) * 4 + key.charCodeAt(2) - 49;
  const dictionary = (input, prefix) => {
    if (input === undefined || input === null) return [identity(), true];
    if (!isObject(input))
      throw new IdlTypeError(`${prefix}The provided value is not of type 'DOMMatrixInit'.`);
    const given = { __proto__: null };
    for (let at = 0; at < matrix2DMembers.length; at++) {
      const key = matrix2DMembers[at];
      given[key] = member(input, key, undefined, prefix, "DOMMatrix2DInit");
    }
    const flat = input.is2D,
      want2D = flat === undefined ? undefined : !!flat;
    for (let at = 0; at < matrix3DMembers.length; at++) {
      const key = matrix3DMembers[at];
      given[key] = member(input, key, undefined, prefix, "DOMMatrixInit");
    }
    const result = identity();
    for (let at = 0; at < matrix2DMembers.length; at++) {
      const key = matrix2DMembers[at];
      if (key.length === 3 && given[key] !== undefined) result[matrixIndex(key)] = given[key];
    }
    for (let at = 0; at < matrix3DMembers.length; at++) {
      const key = matrix3DMembers[at];
      if (given[key] !== undefined) result[matrixIndex(key)] = given[key];
    }
    for (let entry = 0; entry < matrixKeys.length; entry++) {
      const key = matrixKeys[entry][0],
        index = matrixKeys[entry][1];
      if (given[key] === undefined) continue;
      const full = `m${(index >> 2) + 1}${(index % 4) + 1}`;
      if (given[full] !== undefined && !same(given[key], given[full]))
        throw new IdlTypeError("Conflicting matrix values");
      result[index] = given[key];
    }
    if (want2D === true && !is2D(result))
      throw new IdlTypeError(
        `${prefix}The is2D member is set to true but the input matrix is a 3d matrix.`,
      );
    // Without `is2D` the dictionary is 2D unless a 3D member has a non-default value.
    return [result, want2D === undefined ? is2D(result) : want2D];
  };
  const inverse = (values) => {
    // gfx::Transform keeps axis-aligned 2D transforms in float vectors. Its
    // reciprocal scale is rounded before multiplying the translation.
    // A non-invertible matrix becomes all NaN and, like Blink, 3D.
    const singular = () => {
      const result = identity();
      for (let index = 0; index < 16; index++) result[index] = NaN;
      return [result, false];
    };
    if (is2D(values) && values[1] === 0 && values[4] === 0) {
      if (values[0] === 0 || values[5] === 0) return singular();
      const result = identity();
      result[0] = fround(1 / fround(values[0]));
      result[5] = fround(1 / fround(values[5]));
      result[12] = fround(-fround(values[12]) * result[0]);
      result[13] = fround(-fround(values[13]) * result[5]);
      return [result, true];
    }
    const rows = [
      [0, 0, 0, 0, 0, 0, 0, 0],
      [0, 0, 0, 0, 0, 0, 0, 0],
      [0, 0, 0, 0, 0, 0, 0, 0],
      [0, 0, 0, 0, 0, 0, 0, 0],
    ];
    for (let row = 0; row < 4; row++)
      for (let column = 0; column < 8; column++)
        rows[row][column] = column < 4 ? values[column * 4 + row] : column - 4 === row ? 1 : 0;
    for (let column = 0; column < 4; column++) {
      let pivot = column;
      for (let row = column + 1; row < 4; row++)
        if (abs(rows[row][column]) > abs(rows[pivot][column])) pivot = row;
      if (rows[pivot][column] === 0) return singular();
      const swap = rows[pivot];
      rows[pivot] = rows[column];
      rows[column] = swap;
      const factor = rows[column][column];
      for (let index = 0; index < 8; index++) rows[column][index] /= factor;
      for (let row = 0; row < 4; row++)
        if (row !== column) {
          const multiple = rows[row][column];
          for (let index = 0; index < 8; index++)
            rows[row][index] -= multiple * rows[column][index];
        }
    }
    const result = identity();
    for (let index = 0; index < 16; index++) result[index] = rows[index % 4][4 + (index >> 2)];
    return [result, true];
  };
  class DOMPointReadOnly {
    constructor(x = 0, y = 0, z = 0, w = 1) {
      store(points, this, [toNumber(x), toNumber(y), toNumber(z), toNumber(w)]);
    }
    static fromPoint(other = undefined) {
      const result = create(DOMPointReadOnly.prototype);
      store(points, result, pointInit(other, "Failed to execute 'fromPoint' on 'DOMPointReadOnly': "));
      return result;
    }
    matrixTransform(other = undefined) {
      return transformed(dictionary(other, "Failed to execute 'matrixTransform' on 'DOMPointReadOnly': ")[0], point(this));
    }
    toJSON() {
      const value = point(this);
      return { x: value[0], y: value[1], z: value[2], w: value[3] };
    }
  }
  class DOMPoint extends DOMPointReadOnly {
    constructor(x = 0, y = 0, z = 0, w = 1) {
      super(x, y, z, w);
      apply(markWritable, writable, [this]);
    }
    static fromPoint(other = undefined) {
      const value = pointInit(other, "Failed to execute 'fromPoint' on 'DOMPoint': ");
      return newPoint(value[0], value[1], value[2], value[3]);
    }
  }
  const pointKeys = ["x", "y", "z", "w"];
  for (let index = 0; index < 4; index++) {
    const key = pointKeys[index];
    Object.defineProperty(DOMPointReadOnly.prototype, key, {
      get() {
        return point(this)[index];
      },
      enumerable: true,
      configurable: true,
    });
    Object.defineProperty(DOMPoint.prototype, key, {
      get() {
        return point(this)[index];
      },
      set(value) {
        point(this)[index] = toNumber(value);
      },
      enumerable: true,
      configurable: true,
    });
  }
  const sequenceArgument = (count, input) => {
    if (!count) throw new IdlTypeError("An array is required");
    return fromSequence(input);
  };
  // Operations shared by the copying members and the *Self members: each takes a
  // matrix state and returns the new [values, is2D] (Blink's DOMMatrix::*Self).
  const translatedState = (current, tx, ty, tz) => {
    const x = toNumber(tx),
      y = toNumber(ty),
      z = toNumber(tz);
    return [translated(current[0], x, y, z), current[1] && z === 0];
  };
  const scaledState = (current, scaleX, scaleY, scaleZ, originX, originY, originZ) => {
    const sx = toNumber(scaleX),
      sy = toNumber(scaleY),
      sz = toNumber(scaleZ),
      ox = toNumber(originX),
      oy = toNumber(originY),
      oz = toNumber(originZ);
    return [scaled(current[0], sx, sy, sz, ox, oy, oz), current[1] && sz === 1 && oz === 0];
  };
  const rotatedState = (current, rotX, rotY, rotZ) => {
    const result = rotated(current[0], rotX, rotY, rotZ);
    return [result[0], current[1] && result[1]];
  };
  const rotatedAxisAngleState = (current, x, y, z, degrees) => {
    const ax = toNumber(x),
      ay = toNumber(y),
      az = toNumber(z),
      turn = toNumber(degrees);
    return [
      rotatedAxisAngle(current[0], ax, ay, az, turn),
      current[1] && ax === 0 && ay === 0,
    ];
  };
  const fromVectorState = (current, x, y) => [
    rotated(current[0], (atan2(y, x) * 180) / PI)[0],
    current[1],
  ];
  const multipliedState = (left, right) => [multiply(left[0], right[0]), left[1] && right[1]];
  const invertedState = (current) => {
    const result = inverse(current[0]);
    return [result[0], current[1] && result[1]];
  };
  class DOMMatrixReadOnly {
    constructor(init = undefined) {
      const initial =
        init === undefined
          ? [identity(), true]
          : typeof init === "string"
            ? parse(init)
            : fromSequence(init);
      assign(this, initial[0], initial[1]);
    }
    static fromMatrix(other = undefined) {
      const result = dictionary(other, "Failed to execute 'fromMatrix' on 'DOMMatrixReadOnly': ");
      return newMatrix(result[0], result[1], false);
    }
    static fromFloat32Array(input) {
      const result = sequenceArgument(arguments.length, input);
      return newMatrix(result[0], result[1], false);
    }
    static fromFloat64Array(input) {
      const result = sequenceArgument(arguments.length, input);
      return newMatrix(result[0], result[1], false);
    }
    get is2D() {
      return state(this)[1];
    }
    get isIdentity() {
      return isIdentity(matrix(this));
    }
    multiply(other = undefined) {
      const current = state(this);
      const result = multipliedState(
        current,
        dictionary(other, "Failed to execute 'multiply' on 'DOMMatrixReadOnly': "),
      );
      return newMatrix(result[0], result[1]);
    }
    translate(tx = 0, ty = 0, tz = 0) {
      const result = translatedState(state(this), tx, ty, tz);
      return newMatrix(result[0], result[1]);
    }
    scale(scaleX = 1, scaleY = scaleX, scaleZ = 1, originX = 0, originY = 0, originZ = 0) {
      const result = scaledState(state(this), scaleX, scaleY, scaleZ, originX, originY, originZ);
      return newMatrix(result[0], result[1]);
    }
    scaleNonUniform(scaleX = 1, scaleY = 1) {
      const result = scaledState(state(this), scaleX, scaleY, 1, 0, 0, 0);
      return newMatrix(result[0], result[1]);
    }
    scale3d(scale = 1, originX = 0, originY = 0, originZ = 0) {
      const result = scaledState(state(this), scale, scale, scale, originX, originY, originZ);
      return newMatrix(result[0], result[1]);
    }
    rotate(rotX = 0, rotY = undefined, rotZ = undefined) {
      const result = rotatedState(state(this), rotX, rotY, rotZ);
      return newMatrix(result[0], result[1]);
    }
    rotateAxisAngle(x = 0, y = 0, z = 0, degrees = 0) {
      const result = rotatedAxisAngleState(state(this), x, y, z, degrees);
      return newMatrix(result[0], result[1]);
    }
    rotateFromVector(x = 0, y = 0) {
      const result = fromVectorState(state(this), x, y);
      return newMatrix(result[0], result[1]);
    }
    skewX(degrees = 0) {
      const current = state(this);
      return newMatrix(skewed(current[0], 4, degrees), current[1]);
    }
    skewY(degrees = 0) {
      const current = state(this);
      return newMatrix(skewed(current[0], 1, degrees), current[1]);
    }
    flipX() {
      const current = state(this);
      return newMatrix(scaled(current[0], -1, 1, 1, 0, 0, 0), current[1]);
    }
    flipY() {
      const current = state(this);
      return newMatrix(scaled(current[0], 1, -1, 1, 0, 0, 0), current[1]);
    }
    inverse() {
      const result = invertedState(state(this));
      return newMatrix(result[0], result[1]);
    }
    transformPoint(other = undefined) {
      const value = matrix(this);
      return transformed(value, pointInit(other, "Failed to execute 'transformPoint' on 'DOMMatrixReadOnly': "));
    }
    toFloat32Array() {
      const values = matrix(this),
        result = new Float32(16);
      for (let index = 0; index < 16; index++) result[index] = values[index];
      return result;
    }
    toFloat64Array() {
      const values = matrix(this),
        result = new Float64(16);
      for (let index = 0; index < 16; index++) result[index] = values[index];
      return result;
    }
    toJSON() {
      const current = state(this),
        v = current[0];
      return {
        a: v[0],
        b: v[1],
        c: v[4],
        d: v[5],
        e: v[12],
        f: v[13],
        m11: v[0],
        m12: v[1],
        m13: v[2],
        m14: v[3],
        m21: v[4],
        m22: v[5],
        m23: v[6],
        m24: v[7],
        m31: v[8],
        m32: v[9],
        m33: v[10],
        m34: v[11],
        m41: v[12],
        m42: v[13],
        m43: v[14],
        m44: v[15],
        is2D: current[1],
        isIdentity: isIdentity(v),
      };
    }
    toString() {
      const current = state(this),
        values = current[0];
      for (let index = 0; index < 16; index++)
        if (!finite(values[index]))
          throw new IdlDOMException("A matrix contains non-finite values", "InvalidStateError");
      const flat = current[1],
        entries = flat
          ? [values[0], values[1], values[4], values[5], values[12], values[13]]
          : values;
      let text = "";
      for (let index = 0; index < entries.length; index++)
        text += (index ? ", " : "") + entries[index];
      return flat ? `matrix(${text})` : `matrix3d(${text})`;
    }
  }
  class DOMMatrix extends DOMMatrixReadOnly {
    constructor(init = undefined) {
      super(init);
      apply(markWritable, writable, [this]);
    }
    static fromMatrix(other = undefined) {
      const result = dictionary(other, "Failed to execute 'fromMatrix' on 'DOMMatrix': ");
      return newMatrix(result[0], result[1]);
    }
    static fromFloat32Array(input) {
      const result = sequenceArgument(arguments.length, input);
      return newMatrix(result[0], result[1]);
    }
    static fromFloat64Array(input) {
      const result = sequenceArgument(arguments.length, input);
      return newMatrix(result[0], result[1]);
    }
    multiplySelf(other = undefined) {
      const current = state(this);
      const result = multipliedState(
        current,
        dictionary(other, "Failed to execute 'multiplySelf' on 'DOMMatrix': "),
      );
      assign(this, result[0], result[1]);
      return this;
    }
    preMultiplySelf(other = undefined) {
      const init = dictionary(other, "Failed to execute 'preMultiplySelf' on 'DOMMatrix': ");
      const result = multipliedState(init, state(this));
      assign(this, result[0], result[1]);
      return this;
    }
    translateSelf(tx = 0, ty = 0, tz = 0) {
      const result = translatedState(state(this), tx, ty, tz);
      assign(this, result[0], result[1]);
      return this;
    }
    scaleSelf(scaleX = 1, scaleY = scaleX, scaleZ = 1, originX = 0, originY = 0, originZ = 0) {
      const result = scaledState(state(this), scaleX, scaleY, scaleZ, originX, originY, originZ);
      assign(this, result[0], result[1]);
      return this;
    }
    scale3dSelf(scale = 1, originX = 0, originY = 0, originZ = 0) {
      const result = scaledState(state(this), scale, scale, scale, originX, originY, originZ);
      assign(this, result[0], result[1]);
      return this;
    }
    rotateSelf(rotX = 0, rotY = undefined, rotZ = undefined) {
      const result = rotatedState(state(this), rotX, rotY, rotZ);
      assign(this, result[0], result[1]);
      return this;
    }
    rotateAxisAngleSelf(x = 0, y = 0, z = 0, degrees = 0) {
      const result = rotatedAxisAngleState(state(this), x, y, z, degrees);
      assign(this, result[0], result[1]);
      return this;
    }
    rotateFromVectorSelf(x = 0, y = 0) {
      const result = fromVectorState(state(this), x, y);
      assign(this, result[0], result[1]);
      return this;
    }
    skewXSelf(degrees = 0) {
      const current = state(this);
      assign(this, skewed(current[0], 4, degrees), current[1]);
      return this;
    }
    skewYSelf(degrees = 0) {
      const current = state(this);
      assign(this, skewed(current[0], 1, degrees), current[1]);
      return this;
    }
    invertSelf() {
      const result = invertedState(state(this));
      assign(this, result[0], result[1]);
      return this;
    }
    setMatrixValue(source) {
      if (!arguments.length) throw new IdlTypeError("A CSS transform is required");
      const result = parse(Text(source));
      assign(this, result[0], result[1]);
      return this;
    }
  }
  for (let column = 1; column <= 4; column++)
    for (let row = 1; row <= 4; row++) {
      const index = (column - 1) * 4 + row - 1,
        key = `m${column}${row}`,
        // Blink: setting a 3D member to anything but its identity value (NaN
        // included) makes the matrix 3D for good.
        threeD = row > 2 || column === 3,
        unit = column === row ? 1 : 0;
      Object.defineProperty(DOMMatrixReadOnly.prototype, key, {
        get() {
          return matrix(this)[index];
        },
        enumerable: true,
        configurable: true,
      });
      Object.defineProperty(DOMMatrix.prototype, key, {
        get() {
          return matrix(this)[index];
        },
        set(value) {
          const values = matrix(this),
            number = toNumber(value);
          values[index] = number;
          if (threeD && number !== unit) store(flatMatrices, this, false);
        },
        enumerable: true,
        configurable: true,
      });
    }
  for (const [key, index] of matrixKeys) {
    Object.defineProperty(DOMMatrixReadOnly.prototype, key, {
      get() {
        return matrix(this)[index];
      },
      enumerable: true,
      configurable: true,
    });
    Object.defineProperty(DOMMatrix.prototype, key, {
      get() {
        return matrix(this)[index];
      },
      set(value) {
        matrix(this)[index] = toNumber(value);
      },
      enumerable: true,
      configurable: true,
    });
  }
  // DOMQuad (turbo-surf#69): four live DOMPoint platform objects. Dictionaries
  // convert their members in WebIDL's lexicographic order, each read and then
  // converted, with Blink's nested "Failed to read the ..." messages.
  const quads = new WeakMap();
  let quadRectangle = null;
  // geometry_backend.rs installs DOMRect after this bootstrap and, before any page
  // script runs, hands its constructor over once (a page cannot replace it later).
  Object.defineProperty(__G, "installQuadRectangle", {
    __proto__: null,
    value: (constructor) => {
      if (quadRectangle === null && typeof constructor === "function") quadRectangle = constructor;
      delete __G.installQuadRectangle;
    },
    configurable: true,
  });
  const quad = (receiver) => {
    const data = read(quads, receiver);
    if (!data) throw new IdlTypeError("Illegal invocation");
    return data;
  };
  const corners = (values) => [
    newPoint(values[0], values[1], values[2], values[3]),
    newPoint(values[4], values[5], values[6], values[7]),
    newPoint(values[8], values[9], values[10], values[11]),
    newPoint(values[12], values[13], values[14], values[15]),
  ];
  // `values`: the four points' x, y, z, w (16 numbers).
  const newQuad = (values) => {
    const result = create(DOMQuad.prototype);
    store(quads, result, corners(values));
    return result;
  };
  const flatten = (p1, p2, p3, p4) => [
    p1[0],
    p1[1],
    p1[2],
    p1[3],
    p2[0],
    p2[1],
    p2[2],
    p2[3],
    p3[0],
    p3[1],
    p3[2],
    p3[3],
    p4[0],
    p4[1],
    p4[2],
    p4[3],
  ];
  const quadFields = ["p1", "p2", "p3", "p4"];
  // A function (not a class) so a call without `new` reaches Blink's message.
  function DOMQuad(...init) {
    const prefix = "Failed to construct 'DOMQuad': ";
    if (new.target === undefined)
      throw new IdlTypeError(
        `${prefix}Please use the 'new' operator, this DOM object constructor cannot be called as a function.`,
      );
    const values = [];
    for (let index = 0; index < 4; index++) values[index] = pointInit(init[index], prefix);
    store(quads, this, corners(flatten(values[0], values[1], values[2], values[3])));
  }
  const quadMembers = class {
    static fromQuad(...init) {
      const prefix = "Failed to execute 'fromQuad' on 'DOMQuad': ",
        other = init[0];
      if (other !== undefined && other !== null && !isObject(other))
        throw new IdlTypeError(`${prefix}The provided value is not of type 'DOMQuadInit'.`);
      const source = other ?? {},
        values = [];
      for (let index = 0; index < 4; index++) {
        const name = quadFields[index];
        values[index] = pointInit(
          source[name],
          `${prefix}Failed to read the '${name}' property from 'DOMQuadInit': `,
        );
      }
      return newQuad(flatten(values[0], values[1], values[2], values[3]));
    }
    static fromRect(...init) {
      const prefix = "Failed to execute 'fromRect' on 'DOMQuad': ",
        other = init[0];
      if (other !== undefined && other !== null && !isObject(other))
        throw new IdlTypeError(`${prefix}The provided value is not of type 'DOMRectInit'.`);
      const rect = other ?? {},
        height = member(rect, "height", 0, prefix, "DOMRectInit"),
        width = member(rect, "width", 0, prefix, "DOMRectInit"),
        x = member(rect, "x", 0, prefix, "DOMRectInit"),
        y = member(rect, "y", 0, prefix, "DOMRectInit");
      return newQuad(
        flatten(
          [x, y, 0, 1],
          [x + width, y, 0, 1],
          [x + width, y + height, 0, 1],
          [x, y + height, 0, 1],
        ),
      );
    }
    getBounds() {
      const points4 = quad(this),
        xs = [],
        ys = [];
      for (let index = 0; index < 4; index++) {
        const value = point(points4[index]);
        xs[index] = value[0];
        ys[index] = value[1];
      }
      if (quadRectangle === null) throw new IdlTypeError("Illegal invocation");
      const left = min(xs[0], xs[1], xs[2], xs[3]),
        top = min(ys[0], ys[1], ys[2], ys[3]);
      return construct(quadRectangle, [
        left,
        top,
        max(xs[0], xs[1], xs[2], xs[3]) - left,
        max(ys[0], ys[1], ys[2], ys[3]) - top,
      ]);
    }
    toJSON() {
      const value = quad(this);
      return { p1: value[0], p2: value[1], p3: value[2], p4: value[3] };
    }
  };
  for (const key of ["fromQuad", "fromRect"])
    Object.defineProperty(DOMQuad, key, Object.getOwnPropertyDescriptor(quadMembers, key));
  for (let index = 0; index < 4; index++)
    Object.defineProperty(DOMQuad.prototype, quadFields[index], {
      get() {
        return quad(this)[index];
      },
      enumerable: true,
      configurable: true,
    });
  // Chrome lists the attributes first: p1..p4, getBounds, toJSON, constructor.
  for (const key of ["getBounds", "toJSON"])
    Object.defineProperty(
      DOMQuad.prototype,
      key,
      Object.getOwnPropertyDescriptor(quadMembers.prototype, key),
    );
  delete DOMQuad.prototype.constructor;
  Object.defineProperty(DOMQuad.prototype, "constructor", {
    value: DOMQuad,
    writable: true,
    configurable: true,
  });
  Object.defineProperty(DOMQuad, "prototype", { writable: false });
  for (const constructor of [DOMPointReadOnly, DOMPoint, DOMMatrixReadOnly, DOMMatrix, DOMQuad]) {
    Object.defineProperty(constructor.prototype, Symbol.toStringTag, {
      value: constructor.name,
      configurable: true,
    });
    Object.defineProperty(globalThis, constructor.name, {
      value: constructor,
      writable: true,
      configurable: true,
      enumerable: false,
    });
  }
  globalThis.WebKitCSSMatrix = DOMMatrix;
  // Structured clone (crypto_clone.rs): kind 1-4 = DOMPointReadOnly, DOMPoint,
  // DOMMatrixReadOnly, DOMMatrix with the private values (6 for a 2D matrix, 16 for
  // a 3D one), 5 = DOMQuad with its
  // points' x, y, z, w; restore rebuilds the platform object in this realm
  // without running guest-patchable conversions.
  const clonePrototypes = {
    __proto__: null,
    1: DOMPointReadOnly.prototype,
    2: DOMPoint.prototype,
    3: DOMMatrixReadOnly.prototype,
    4: DOMMatrix.prototype,
  };
  const cloneRecord = (value) => {
    const quadPoints = read(quads, value);
    if (quadPoints !== undefined) {
      const p1 = point(quadPoints[0]),
        p2 = point(quadPoints[1]),
        p3 = point(quadPoints[2]),
        p4 = point(quadPoints[3]);
      return [5, flatten(p1, p2, p3, p4)];
    }
    const mutable = apply(isWritable, writable, [value]) ? 1 : 0;
    const coordinates = read(points, value);
    if (coordinates !== undefined) return [1 + mutable, coordinates];
    const values = read(matrices, value);
    // Like Blink's serializer, a 2D matrix travels as its 6 2D values.
    if (values !== undefined)
      return [
        3 + mutable,
        read(flatMatrices, value) === true
          ? [values[0], values[1], values[4], values[5], values[12], values[13]]
          : values,
      ];
  };
  const cloneRestore = (kind, values) => {
    if (kind === 5) return newQuad(values);
    const result = create(clonePrototypes[kind]);
    if (kind <= 2) store(points, result, values);
    else assign(result, expand(values), values.length === 6);
    if (kind % 2 === 0) apply(markWritable, writable, [result]);
    return result;
  };
  ops.op_install_geometry_clone_adapter(cloneRecord, cloneRestore);
})();
