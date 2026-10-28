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
    points = new WeakMap(),
    // DOMPoint/DOMMatrix platform objects (not their read-only bases), recorded by the
    // interface constructor that ran, as Chrome serializes the wrapper's interface.
    writable = new WeakSet();
  const apply = Reflect.apply,
    create = Object.create,
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
    { abs, atan2, cos, fround, hypot, sin, tan, PI } = Math;
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
  const newMatrix = (values, mutable = true) => {
    const result = create(mutable ? DOMMatrix.prototype : DOMMatrixReadOnly.prototype);
    store(matrices, result, values);
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
  const fromSequence = (input) => {
    const values = arrayFrom(input, toNumber);
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
  const angleUnits = { __proto__: null, deg: PI / 180, rad: 1, grad: PI / 200, turn: 2 * PI };
  const angle = (input) => {
    const match = /^([+-]?(?:\d+(?:\.\d*)?|\.\d+)(?:e[+-]?\d+)?)(deg|rad|grad|turn)?$/i.exec(input);
    if (!match || (!match[2] && toNumber(match[1]) !== 0))
      throw new IdlDOMException("Invalid transform angle", "SyntaxError");
    return toNumber(match[1]) * (angleUnits[match[2]?.toLowerCase()] || 1);
  };
  const length = (input) => {
    const match = /^([+-]?(?:\d+(?:\.\d*)?|\.\d+)(?:e[+-]?\d+)?)(px)?$/i.exec(input);
    if (!match || (!match[2] && toNumber(match[1]) !== 0))
      throw new IdlDOMException("Invalid absolute transform length", "SyntaxError");
    return toNumber(match[1]);
  };
  const rotation = (x, y, z, radians) => {
    const magnitude = hypot(x, y, z);
    if (!magnitude) return identity();
    x /= magnitude;
    y /= magnitude;
    z /= magnitude;
    const c = cos(radians),
      s = sin(radians),
      t = 1 - c;
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
      rotation(toNumber(x), toNumber(y), toNumber(z), (toNumber(degrees) * PI) / 180),
    );
  const rotated = (values, rotX, rotY, rotZ) => {
    if (rotY === undefined && rotZ === undefined) {
      rotZ = rotX;
      rotX = 0;
      rotY = 0;
    }
    const z = rotatedAxisAngle(values, 0, 0, 1, rotZ ?? 0);
    return rotatedAxisAngle(rotatedAxisAngle(z, 0, 1, 0, rotY ?? 0), 1, 0, 0, rotX);
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
  const transformFunction = (name, args) => {
    const value = identity();
    if (name === "matrix" || name === "matrix3d") {
      if (args.length !== (name === "matrix" ? 6 : 16))
        throw new IdlDOMException("Invalid matrix", "SyntaxError");
      return fromSequence(transformArguments(numbers(args)));
    }
    if (/^translate[xyz]?$/.test(name) || name === "translate3d") {
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
    if (/^scale[xyz]?$/.test(name) || name === "scale3d") {
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
    if (/^rotate[xyz]?$/.test(name)) {
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
      if (name !== "skewy") value[4] = tan(angle(args[0]));
      if (name === "skewy") value[1] = tan(angle(args[0]));
      else if (args[1]) value[1] = tan(angle(args[1]));
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
  const parse = (source) => {
    source = source.trim();
    if (!source || source === "none") return identity();
    let result = identity(),
      end = 0;
    const matches = source.matchAll(/([a-zA-Z0-9]+)\(([^()]*)\)/g);
    for (const match of matches) {
      if (source.slice(end, match.index).trim())
        throw new IdlDOMException("Invalid CSS transform", "SyntaxError");
      end = match.index + match[0].length;
      const parts = match[2].trim().split(/\s*,\s*|\s+/),
        args = [];
      for (let index = 0; index < parts.length; index++)
        if (parts[index]) args[args.length] = parts[index];
      result = multiply(result, transformFunction(match[1].toLowerCase(), args));
    }
    if (source.slice(end).trim() || !end)
      throw new IdlDOMException("Invalid CSS transform", "SyntaxError");
    return result;
  };
  const matrixKeys = [
    ["a", 0],
    ["b", 1],
    ["c", 4],
    ["d", 5],
    ["e", 12],
    ["f", 13],
  ];
  const dictionary = (input) => {
    const result = identity();
    for (let column = 1; column <= 4; column++)
      for (let row = 1; row <= 4; row++) {
        const key = `m${column}${row}`;
        if (input[key] !== undefined) result[(column - 1) * 4 + row - 1] = toNumber(input[key]);
      }
    for (let entry = 0; entry < matrixKeys.length; entry++) {
      const key = matrixKeys[entry][0],
        index = matrixKeys[entry][1];
      if (input[key] !== undefined) {
        const full = `m${(index >> 2) + 1}${(index % 4) + 1}`;
        if (input[full] !== undefined && !same(toNumber(input[key]), toNumber(input[full])))
          throw new IdlTypeError("Conflicting matrix values");
        result[index] = toNumber(input[key]);
      }
    }
    if (input.is2D === true && !is2D(result))
      throw new IdlTypeError("A 2D matrix cannot contain 3D components");
    return result;
  };
  const inverse = (values) => {
    // gfx::Transform keeps axis-aligned 2D transforms in float vectors. Its
    // reciprocal scale is rounded before multiplying the translation.
    const singular = () => {
      const result = identity();
      for (let index = 0; index < 16; index++) result[index] = NaN;
      return result;
    };
    if (is2D(values) && values[1] === 0 && values[4] === 0) {
      if (values[0] === 0 || values[5] === 0) return singular();
      const result = identity();
      result[0] = fround(1 / fround(values[0]));
      result[5] = fround(1 / fround(values[5]));
      result[12] = fround(-fround(values[12]) * result[0]);
      result[13] = fround(-fround(values[13]) * result[5]);
      return result;
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
    return result;
  };
  const pointFrom = (other) => [
    toNumber(other.x ?? 0),
    toNumber(other.y ?? 0),
    toNumber(other.z ?? 0),
    toNumber(other.w ?? 1),
  ];
  class DOMPointReadOnly {
    constructor(x = 0, y = 0, z = 0, w = 1) {
      store(points, this, [toNumber(x), toNumber(y), toNumber(z), toNumber(w)]);
    }
    static fromPoint(other = {}) {
      const result = create(DOMPointReadOnly.prototype);
      store(points, result, pointFrom(other));
      return result;
    }
    matrixTransform(other = {}) {
      return transformed(dictionary(other), point(this));
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
    static fromPoint(other = {}) {
      const value = pointFrom(other);
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
  class DOMMatrixReadOnly {
    constructor(init = undefined) {
      store(
        matrices,
        this,
        init === undefined
          ? identity()
          : typeof init === "string"
            ? parse(init)
            : fromSequence(init),
      );
    }
    static fromMatrix(other = {}) {
      return newMatrix(dictionary(other), false);
    }
    static fromFloat32Array(input) {
      return newMatrix(sequenceArgument(arguments.length, input), false);
    }
    static fromFloat64Array(input) {
      return newMatrix(sequenceArgument(arguments.length, input), false);
    }
    get is2D() {
      return is2D(matrix(this));
    }
    get isIdentity() {
      return isIdentity(matrix(this));
    }
    multiply(other = {}) {
      return newMatrix(multiply(matrix(this), dictionary(other)));
    }
    translate(tx = 0, ty = 0, tz = 0) {
      return newMatrix(translated(matrix(this), tx, ty, tz));
    }
    scale(scaleX = 1, scaleY = scaleX, scaleZ = 1, originX = 0, originY = 0, originZ = 0) {
      return newMatrix(scaled(matrix(this), scaleX, scaleY, scaleZ, originX, originY, originZ));
    }
    scaleNonUniform(scaleX = 1, scaleY = 1) {
      return newMatrix(scaled(matrix(this), scaleX, scaleY, 1, 0, 0, 0));
    }
    scale3d(scale = 1, originX = 0, originY = 0, originZ = 0) {
      return newMatrix(scaled(matrix(this), scale, scale, scale, originX, originY, originZ));
    }
    rotate(rotX = 0, rotY = undefined, rotZ = undefined) {
      return newMatrix(rotated(matrix(this), rotX, rotY, rotZ));
    }
    rotateAxisAngle(x = 0, y = 0, z = 0, degrees = 0) {
      return newMatrix(rotatedAxisAngle(matrix(this), x, y, z, degrees));
    }
    rotateFromVector(x = 0, y = 0) {
      return newMatrix(rotated(matrix(this), (atan2(y, x) * 180) / PI));
    }
    skewX(degrees = 0) {
      return newMatrix(skewed(matrix(this), 4, degrees));
    }
    skewY(degrees = 0) {
      return newMatrix(skewed(matrix(this), 1, degrees));
    }
    flipX() {
      return newMatrix(scaled(matrix(this), -1, 1, 1, 0, 0, 0));
    }
    flipY() {
      return newMatrix(scaled(matrix(this), 1, -1, 1, 0, 0, 0));
    }
    inverse() {
      return newMatrix(inverse(matrix(this)));
    }
    transformPoint(other = {}) {
      const value = matrix(this);
      return transformed(value, pointFrom(other));
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
      const v = matrix(this);
      return {
        a: v[0],
        b: v[1],
        c: v[4],
        d: v[5],
        e: v[12],
        f: v[13],
        is2D: is2D(v),
        isIdentity: isIdentity(v),
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
      };
    }
    toString() {
      const values = matrix(this);
      for (let index = 0; index < 16; index++)
        if (!finite(values[index]))
          throw new IdlDOMException("A matrix contains non-finite values", "InvalidStateError");
      const flat = is2D(values),
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
    static fromMatrix(other = {}) {
      return newMatrix(dictionary(other));
    }
    static fromFloat32Array(input) {
      return newMatrix(sequenceArgument(arguments.length, input));
    }
    static fromFloat64Array(input) {
      return newMatrix(sequenceArgument(arguments.length, input));
    }
    multiplySelf(other = {}) {
      store(matrices, this, multiply(matrix(this), dictionary(other)));
      return this;
    }
    preMultiplySelf(other = {}) {
      store(matrices, this, multiply(dictionary(other), matrix(this)));
      return this;
    }
    translateSelf(tx = 0, ty = 0, tz = 0) {
      store(matrices, this, translated(matrix(this), tx, ty, tz));
      return this;
    }
    scaleSelf(scaleX = 1, scaleY = scaleX, scaleZ = 1, originX = 0, originY = 0, originZ = 0) {
      store(
        matrices,
        this,
        scaled(matrix(this), scaleX, scaleY, scaleZ, originX, originY, originZ),
      );
      return this;
    }
    scale3dSelf(scale = 1, originX = 0, originY = 0, originZ = 0) {
      store(matrices, this, scaled(matrix(this), scale, scale, scale, originX, originY, originZ));
      return this;
    }
    rotateSelf(rotX = 0, rotY = undefined, rotZ = undefined) {
      store(matrices, this, rotated(matrix(this), rotX, rotY, rotZ));
      return this;
    }
    rotateAxisAngleSelf(x = 0, y = 0, z = 0, degrees = 0) {
      store(matrices, this, rotatedAxisAngle(matrix(this), x, y, z, degrees));
      return this;
    }
    rotateFromVectorSelf(x = 0, y = 0) {
      store(matrices, this, rotated(matrix(this), (atan2(y, x) * 180) / PI));
      return this;
    }
    skewXSelf(degrees = 0) {
      store(matrices, this, skewed(matrix(this), 4, degrees));
      return this;
    }
    skewYSelf(degrees = 0) {
      store(matrices, this, skewed(matrix(this), 1, degrees));
      return this;
    }
    invertSelf() {
      store(matrices, this, inverse(matrix(this)));
      return this;
    }
    setMatrixValue(source) {
      if (!arguments.length) throw new IdlTypeError("A CSS transform is required");
      store(matrices, this, parse(Text(source)));
      return this;
    }
  }
  for (let column = 1; column <= 4; column++)
    for (let row = 1; row <= 4; row++) {
      const index = (column - 1) * 4 + row - 1,
        key = `m${column}${row}`;
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
  for (const constructor of [DOMPointReadOnly, DOMPoint, DOMMatrixReadOnly, DOMMatrix]) {
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
  // DOMMatrixReadOnly, DOMMatrix with the private values; restore rebuilds the
  // platform object in this realm without running guest-patchable conversions.
  const clonePrototypes = {
    __proto__: null,
    1: DOMPointReadOnly.prototype,
    2: DOMPoint.prototype,
    3: DOMMatrixReadOnly.prototype,
    4: DOMMatrix.prototype,
  };
  const cloneRecord = (value) => {
    const mutable = apply(isWritable, writable, [value]) ? 1 : 0;
    const coordinates = read(points, value);
    if (coordinates !== undefined) return [1 + mutable, coordinates];
    const values = read(matrices, value);
    if (values !== undefined) return [3 + mutable, values];
  };
  const cloneRestore = (kind, values) => {
    const result = create(clonePrototypes[kind]);
    store(kind <= 2 ? points : matrices, result, values);
    if (kind % 2 === 0) apply(markWritable, writable, [result]);
    return result;
  };
  ops.op_install_geometry_clone_adapter(cloneRecord, cloneRestore);
})();
