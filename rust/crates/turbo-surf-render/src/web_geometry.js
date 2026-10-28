// Geometry interfaces operate on private column-major matrices. The browser
// environment binds these delegates with native FunctionTemplates afterward.
// The pure helpers stay inside this closure so no name leaks into the shared
// bootstrap scope this file is concatenated into.
// oxlint-disable unicorn/consistent-function-scoping
(() => {
  const matrices = new WeakMap(),
    points = new WeakMap(),
    // DOMPoint/DOMMatrix platform objects (not their read-only bases), recorded by the
    // interface constructor that ran, as Chrome serializes the wrapper's interface.
    writable = new WeakSet();
  const apply = Reflect.apply,
    create = Object.create,
    weakGet = WeakMap.prototype.get,
    weakSet = WeakMap.prototype.set,
    isWritable = WeakSet.prototype.has,
    markWritable = WeakSet.prototype.add;
  const identity = () => [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];
  const matrix = (receiver) => {
    const data = matrices.get(receiver);
    if (!data) throw new TypeError("Illegal invocation");
    return data;
  };
  const point = (receiver) => {
    const data = points.get(receiver);
    if (!data) throw new TypeError("Illegal invocation");
    return data;
  };
  const multiply = (a, b) =>
    Array.from({ length: 16 }, (_, index) => {
      const column = index >> 2,
        row = index % 4;
      return (
        a[row] * b[column * 4] +
        a[4 + row] * b[column * 4 + 1] +
        a[8 + row] * b[column * 4 + 2] +
        a[12 + row] * b[column * 4 + 3]
      );
    });
  const is2D = (value) =>
    [2, 3, 6, 7, 8, 9, 11, 14].every((index) => value[index] === 0) &&
    value[10] === 1 &&
    value[15] === 1;
  const fromSequence = (input) => {
    const values = Array.from(input, Number);
    if (values.length === 16) return values;
    if (values.length !== 6) throw new TypeError("A matrix requires 6 or 16 values");
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
  const angle = (input) => {
    const match = /^([+-]?(?:\d+(?:\.\d*)?|\.\d+)(?:e[+-]?\d+)?)(deg|rad|grad|turn)?$/i.exec(input);
    if (!match || (!match[2] && Number(match[1]) !== 0))
      throw new DOMException("Invalid transform angle", "SyntaxError");
    return (
      Number(match[1]) *
      ({ deg: Math.PI / 180, rad: 1, grad: Math.PI / 200, turn: 2 * Math.PI }[
        match[2]?.toLowerCase()
      ] || 1)
    );
  };
  const length = (input) => {
    const match = /^([+-]?(?:\d+(?:\.\d*)?|\.\d+)(?:e[+-]?\d+)?)(px)?$/i.exec(input);
    if (!match || (!match[2] && Number(match[1]) !== 0))
      throw new DOMException("Invalid absolute transform length", "SyntaxError");
    return Number(match[1]);
  };
  const rotation = (x, y, z, radians) => {
    const magnitude = Math.hypot(x, y, z);
    if (!magnitude) return identity();
    x /= magnitude;
    y /= magnitude;
    z /= magnitude;
    const c = Math.cos(radians),
      s = Math.sin(radians),
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
  const parse = (source) => {
    source = source.trim();
    if (!source || source === "none") return identity();
    let result = identity(),
      end = 0;
    const matches = source.matchAll(/([a-zA-Z0-9]+)\(([^()]*)\)/g);
    for (const match of matches) {
      if (source.slice(end, match.index).trim())
        throw new DOMException("Invalid CSS transform", "SyntaxError");
      end = match.index + match[0].length;
      const name = match[1].toLowerCase(),
        args = match[2]
          .trim()
          .split(/\s*,\s*|\s+/)
          .filter(Boolean);
      let value = identity();
      const numeric = () => {
        const values = args.map(Number);
        if (values.some((n) => !Number.isFinite(n)))
          throw new DOMException("Invalid transform number", "SyntaxError");
        return values;
      };
      if (name === "matrix" || name === "matrix3d") {
        if (args.length !== (name === "matrix" ? 6 : 16))
          throw new DOMException("Invalid matrix", "SyntaxError");
        value = fromSequence(numeric());
      } else if (/^translate[xyz]?$/.test(name) || name === "translate3d") {
        const counts =
          name === "translate"
            ? args.length >= 1 && args.length <= 2
            : name === "translate3d"
              ? args.length === 3
              : args.length === 1;
        if (!counts) throw new DOMException("Invalid translation", "SyntaxError");
        const numbers = args.map(length);
        if (name === "translatex") value[12] = numbers[0];
        else if (name === "translatey") value[13] = numbers[0];
        else if (name === "translatez") value[14] = numbers[0];
        else {
          value[12] = numbers[0];
          value[13] = numbers[1] || 0;
          value[14] = numbers[2] || 0;
        }
      } else if (/^scale[xyz]?$/.test(name) || name === "scale3d") {
        if (
          !(name === "scale"
            ? args.length >= 1 && args.length <= 2
            : name === "scale3d"
              ? args.length === 3
              : args.length === 1)
        )
          throw new DOMException("Invalid scaling", "SyntaxError");
        const numbers = numeric();
        if (name === "scalex") value[0] = numbers[0];
        else if (name === "scaley") value[5] = numbers[0];
        else if (name === "scalez") value[10] = numbers[0];
        else {
          value[0] = numbers[0];
          value[5] = numbers[1] ?? numbers[0];
          value[10] = numbers[2] ?? 1;
        }
      } else if (/^rotate[xyz]?$/.test(name)) {
        if (args.length !== 1) throw new DOMException("Invalid rotation", "SyntaxError");
        value = rotation(
          name === "rotatex" ? 1 : 0,
          name === "rotatey" ? 1 : 0,
          name === "rotatex" || name === "rotatey" ? 0 : 1,
          angle(args[0]),
        );
      } else if (name === "rotate3d") {
        if (args.length !== 4) throw new DOMException("Invalid rotation", "SyntaxError");
        const xyz = args.slice(0, 3).map(Number);
        if (xyz.some((n) => !Number.isFinite(n)))
          throw new DOMException("Invalid rotation", "SyntaxError");
        value = rotation(...xyz, angle(args[3]));
      } else if (name === "skew" || name === "skewx" || name === "skewy") {
        if (!(name === "skew" ? args.length >= 1 && args.length <= 2 : args.length === 1))
          throw new DOMException("Invalid skew", "SyntaxError");
        if (name !== "skewy") value[4] = Math.tan(angle(args[0]));
        if (name === "skewy") value[1] = Math.tan(angle(args[0]));
        else if (args[1]) value[1] = Math.tan(angle(args[1]));
      } else if (name === "perspective" && args.length === 1) {
        const distance = length(args[0]);
        if (distance <= 0) throw new DOMException("Invalid perspective", "SyntaxError");
        value[11] = -1 / distance;
      } else throw new DOMException("Unsupported CSS transform", "SyntaxError");
      result = multiply(result, value);
    }
    if (source.slice(end).trim() || !end)
      throw new DOMException("Invalid CSS transform", "SyntaxError");
    return result;
  };
  const dictionary = (input) => {
    const result = identity();
    for (let column = 1; column <= 4; column++)
      for (let row = 1; row <= 4; row++) {
        const key = `m${column}${row}`;
        if (input[key] !== undefined) result[(column - 1) * 4 + row - 1] = Number(input[key]);
      }
    for (const [key, index] of Object.entries({ a: 0, b: 1, c: 4, d: 5, e: 12, f: 13 }))
      if (input[key] !== undefined) {
        const full = `m${(index >> 2) + 1}${(index % 4) + 1}`;
        if (input[full] !== undefined && !Object.is(Number(input[key]), Number(input[full])))
          throw new TypeError("Conflicting matrix values");
        result[index] = Number(input[key]);
      }
    if (input.is2D === true && !is2D(result))
      throw new TypeError("A 2D matrix cannot contain 3D components");
    return result;
  };
  const inverse = (values) => {
    // gfx::Transform keeps axis-aligned 2D transforms in float vectors. Its
    // reciprocal scale is rounded before multiplying the translation.
    if (is2D(values) && values[1] === 0 && values[4] === 0) {
      if (values[0] === 0 || values[5] === 0) return Array(16).fill(NaN);
      const result = identity();
      result[0] = Math.fround(1 / Math.fround(values[0]));
      result[5] = Math.fround(1 / Math.fround(values[5]));
      result[12] = Math.fround(-Math.fround(values[12]) * result[0]);
      result[13] = Math.fround(-Math.fround(values[13]) * result[5]);
      return result;
    }
    const rows = Array.from({ length: 4 }, (_, row) =>
      Array.from({ length: 8 }, (__, column) =>
        column < 4 ? values[column * 4 + row] : Number(column - 4 === row),
      ),
    );
    for (let column = 0; column < 4; column++) {
      let pivot = column;
      for (let row = column + 1; row < 4; row++)
        if (Math.abs(rows[row][column]) > Math.abs(rows[pivot][column])) pivot = row;
      if (rows[pivot][column] === 0) return Array(16).fill(NaN);
      [rows[pivot], rows[column]] = [rows[column], rows[pivot]];
      const factor = rows[column][column];
      rows[column] = rows[column].map((value) => value / factor);
      for (let row = 0; row < 4; row++)
        if (row !== column) {
          const multiple = rows[row][column];
          rows[row] = rows[row].map((value, index) => value - multiple * rows[column][index]);
        }
    }
    return Array.from({ length: 16 }, (_, index) => rows[index % 4][4 + (index >> 2)]);
  };
  class DOMPointReadOnly {
    constructor(x = 0, y = 0, z = 0, w = 1) {
      points.set(this, [Number(x), Number(y), Number(z), Number(w)]);
    }
    static fromPoint(other = {}) {
      return new this(other.x ?? 0, other.y ?? 0, other.z ?? 0, other.w ?? 1);
    }
    matrixTransform(other = {}) {
      return DOMMatrix.fromMatrix(other).transformPoint(this);
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
      return super.fromPoint(other);
    }
  }
  for (const [index, key] of ["x", "y", "z", "w"].entries()) {
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
        point(this)[index] = Number(value);
      },
      enumerable: true,
      configurable: true,
    });
  }
  class DOMMatrixReadOnly {
    constructor(init = undefined) {
      matrices.set(
        this,
        init === undefined
          ? identity()
          : typeof init === "string"
            ? parse(init)
            : fromSequence(init),
      );
    }
    static fromMatrix(other = {}) {
      const result = new this();
      matrices.set(result, dictionary(other));
      return result;
    }
    static fromFloat32Array(input) {
      if (!arguments.length) throw new TypeError("An array is required");
      return new this(input);
    }
    static fromFloat64Array(input) {
      if (!arguments.length) throw new TypeError("An array is required");
      return new this(input);
    }
    get is2D() {
      return is2D(matrix(this));
    }
    get isIdentity() {
      return matrix(this).every((value, index) => value === identity()[index]);
    }
    multiply(other = {}) {
      const result = new DOMMatrix();
      matrices.set(result, multiply(matrix(this), dictionary(other)));
      return result;
    }
    translate(tx = 0, ty = 0, tz = 0) {
      return new DOMMatrix(matrix(this)).translateSelf(tx, ty, tz);
    }
    scale(scaleX = 1, scaleY = scaleX, scaleZ = 1, originX = 0, originY = 0, originZ = 0) {
      return new DOMMatrix(matrix(this)).scaleSelf(
        scaleX,
        scaleY,
        scaleZ,
        originX,
        originY,
        originZ,
      );
    }
    scaleNonUniform(scaleX = 1, scaleY = 1) {
      return this.scale(scaleX, scaleY);
    }
    scale3d(scale = 1, originX = 0, originY = 0, originZ = 0) {
      return this.scale(scale, scale, scale, originX, originY, originZ);
    }
    rotate(rotX = 0, rotY = undefined, rotZ = undefined) {
      return new DOMMatrix(matrix(this)).rotateSelf(rotX, rotY, rotZ);
    }
    rotateAxisAngle(x = 0, y = 0, z = 0, degrees = 0) {
      return new DOMMatrix(matrix(this)).rotateAxisAngleSelf(x, y, z, degrees);
    }
    rotateFromVector(x = 0, y = 0) {
      return this.rotate((Math.atan2(y, x) * 180) / Math.PI);
    }
    skewX(degrees = 0) {
      return new DOMMatrix(matrix(this)).skewXSelf(degrees);
    }
    skewY(degrees = 0) {
      return new DOMMatrix(matrix(this)).skewYSelf(degrees);
    }
    flipX() {
      return this.scale(-1, 1);
    }
    flipY() {
      return this.scale(1, -1);
    }
    inverse() {
      const result = new DOMMatrix();
      matrices.set(result, inverse(matrix(this)));
      return result;
    }
    transformPoint(other = {}) {
      const value = matrix(this),
        p = [
          Number(other.x ?? 0),
          Number(other.y ?? 0),
          Number(other.z ?? 0),
          Number(other.w ?? 1),
        ];
      return new DOMPoint(
        ...Array.from(
          { length: 4 },
          (_, row) =>
            value[row] * p[0] +
            value[4 + row] * p[1] +
            value[8 + row] * p[2] +
            value[12 + row] * p[3],
        ),
      );
    }
    toFloat32Array() {
      return new Float32Array(matrix(this));
    }
    toFloat64Array() {
      return new Float64Array(matrix(this));
    }
    toJSON() {
      const result = {
        a: this.a,
        b: this.b,
        c: this.c,
        d: this.d,
        e: this.e,
        f: this.f,
        is2D: this.is2D,
        isIdentity: this.isIdentity,
      };
      for (let column = 1; column <= 4; column++)
        for (let row = 1; row <= 4; row++) result[`m${column}${row}`] = this[`m${column}${row}`];
      return result;
    }
    toString() {
      const values = matrix(this);
      if (values.some((value) => !Number.isFinite(value)))
        throw new DOMException("A matrix contains non-finite values", "InvalidStateError");
      return this.is2D
        ? `matrix(${[values[0], values[1], values[4], values[5], values[12], values[13]].join(", ")})`
        : `matrix3d(${values.join(", ")})`;
    }
  }
  class DOMMatrix extends DOMMatrixReadOnly {
    constructor(init = undefined) {
      super(init);
      apply(markWritable, writable, [this]);
    }
    static fromMatrix(other = {}) {
      return super.fromMatrix(other);
    }
    static fromFloat32Array(input) {
      return super.fromFloat32Array(input);
    }
    static fromFloat64Array(input) {
      return super.fromFloat64Array(input);
    }
    multiplySelf(other = {}) {
      matrices.set(this, multiply(matrix(this), dictionary(other)));
      return this;
    }
    preMultiplySelf(other = {}) {
      matrices.set(this, multiply(dictionary(other), matrix(this)));
      return this;
    }
    translateSelf(tx = 0, ty = 0, tz = 0) {
      const value = identity();
      value[12] = Number(tx);
      value[13] = Number(ty);
      value[14] = Number(tz);
      matrices.set(this, multiply(matrix(this), value));
      return this;
    }
    scaleSelf(scaleX = 1, scaleY = scaleX, scaleZ = 1, originX = 0, originY = 0, originZ = 0) {
      const value = identity();
      value[0] = Number(scaleX);
      value[5] = Number(scaleY);
      value[10] = Number(scaleZ);
      this.translateSelf(originX, originY, originZ);
      matrices.set(this, multiply(matrix(this), value));
      return this.translateSelf(-originX, -originY, -originZ);
    }
    scale3dSelf(scale = 1, originX = 0, originY = 0, originZ = 0) {
      return this.scaleSelf(scale, scale, scale, originX, originY, originZ);
    }
    rotateSelf(rotX = 0, rotY = undefined, rotZ = undefined) {
      if (rotY === undefined && rotZ === undefined) {
        rotZ = rotX;
        rotX = 0;
        rotY = 0;
      }
      this.rotateAxisAngleSelf(0, 0, 1, rotZ ?? 0);
      this.rotateAxisAngleSelf(0, 1, 0, rotY ?? 0);
      return this.rotateAxisAngleSelf(1, 0, 0, rotX);
    }
    rotateAxisAngleSelf(x = 0, y = 0, z = 0, degrees = 0) {
      matrices.set(
        this,
        multiply(
          matrix(this),
          rotation(Number(x), Number(y), Number(z), (Number(degrees) * Math.PI) / 180),
        ),
      );
      return this;
    }
    rotateFromVectorSelf(x = 0, y = 0) {
      return this.rotateSelf((Math.atan2(y, x) * 180) / Math.PI);
    }
    skewXSelf(degrees = 0) {
      const value = identity();
      value[4] = Math.tan((Number(degrees) * Math.PI) / 180);
      matrices.set(this, multiply(matrix(this), value));
      return this;
    }
    skewYSelf(degrees = 0) {
      const value = identity();
      value[1] = Math.tan((Number(degrees) * Math.PI) / 180);
      matrices.set(this, multiply(matrix(this), value));
      return this;
    }
    invertSelf() {
      matrices.set(this, inverse(matrix(this)));
      return this;
    }
    setMatrixValue(source) {
      if (!arguments.length) throw new TypeError("A CSS transform is required");
      matrices.set(this, parse(String(source)));
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
          matrix(this)[index] = Number(value);
        },
        enumerable: true,
        configurable: true,
      });
    }
  for (const [key, index] of Object.entries({ a: 0, b: 1, c: 4, d: 5, e: 12, f: 13 })) {
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
        matrix(this)[index] = Number(value);
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
    const coordinates = apply(weakGet, points, [value]);
    if (coordinates !== undefined) return [1 + mutable, coordinates];
    const values = apply(weakGet, matrices, [value]);
    if (values !== undefined) return [3 + mutable, values];
  };
  const cloneRestore = (kind, values) => {
    const result = create(clonePrototypes[kind]);
    apply(weakSet, kind <= 2 ? points : matrices, [result, values]);
    if (kind % 2 === 0) apply(markWritable, writable, [result]);
    return result;
  };
  ops.op_install_geometry_clone_adapter(cloneRecord, cloneRestore);
})();
