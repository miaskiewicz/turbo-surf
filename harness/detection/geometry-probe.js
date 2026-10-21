// Shared mathematical contracts; no network or search-engine interaction.
(() => {
  const close = (value) => Number(value.toFixed(8));
  const matrix = new DOMMatrix("translate(10px, 20px) scale(2, 3)");
  const point = matrix.transformPoint({ x: 4, y: 5 });
  const roundTrip = matrix.inverse().transformPoint(point);
  const ro = new DOMMatrixReadOnly([1, 2, 3, 4, 5, 6]);
  const product = matrix.multiply(new DOMMatrix().translate(7, 8));
  const rotation = new DOMMatrix().rotate(90).transformPoint({ x: 1, y: 0 });
  const singular = new DOMMatrix([0, 0, 0, 0, 0, 0]).inverse();
  const errors = [];
  for (const call of [
    () => new DOMMatrix([1, 2]),
    () => new DOMMatrix("translate(10%, 0)"),
    () => DOMMatrixReadOnly.prototype.toFloat64Array.call({}),
    () => new DOMMatrix("not a transform"),
  ]) {
    try {
      call();
      errors.push(null);
    } catch (error) {
      errors.push(error.name);
    }
  }
  const badReceiver = Object.getOwnPropertyDescriptor(DOMMatrix.prototype, "a").get;
  let illegal = false;
  try {
    badReceiver.call({});
  } catch (error) {
    illegal = error instanceof TypeError;
  }
  return JSON.stringify({
    alias: WebKitCSSMatrix === DOMMatrix,
    private: Reflect.ownKeys(matrix),
    matrix: Array.from(matrix.toFloat64Array()),
    inverse: Array.from(matrix.inverse().toFloat64Array()),
    point: point.toJSON(),
    roundTrip: roundTrip.toJSON(),
    product: [product.a, product.d, product.e, product.f],
    rotation: [close(rotation.x), close(rotation.y)],
    readonly: [ro.a, ro.b, ro.c, ro.d, ro.e, ro.f],
    singular: [Number.isNaN(singular.a), singular.is2D],
    errors,
    illegal,
  });
})();
