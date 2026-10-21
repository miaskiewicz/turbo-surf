// Validate shared operands before either backend performs any work.
const nonemptyString = (value) => typeof value === "string" && value.length > 0;
const operands = {
  __proto__: null,
  type: (value) => typeof value === "string",
  wait: (value) => Number.isSafeInteger(value) && value >= 0 && value <= 60000,
  click: nonemptyString,
  focus: nonemptyString,
  press: nonemptyString,
  move: (value) =>
    value !== null &&
    typeof value === "object" &&
    !Array.isArray(value) &&
    Object.keys(value).length === 1 &&
    Object.hasOwn(value, "selector") &&
    nonemptyString(value.selector),
};

export function validateSteps(steps) {
  if (!Array.isArray(steps)) throw new TypeError("Steps must be an array");
  for (const [index, step] of steps.entries()) {
    if (step === null || typeof step !== "object" || Array.isArray(step))
      throw new TypeError(`Step ${index} must have one supported action`);
    const keys = Object.keys(step);
    const check = operands[keys[0]];
    if (keys.length !== 1 || !check)
      throw new TypeError(`Step ${index} must have one supported action`);
    if (!check(step[keys[0]]))
      throw new TypeError(`Step ${index} has an invalid ${keys[0]} operand`);
  }
}
