// Imported test files may mix node:test probes with Playwright declarations.
// The attempt worker only owns the latter; the outer runner owns Node tests.
const noop = () => {};
export const test = Object.assign(noop, { skip: noop, only: noop, todo: noop });
export const describe = test;
export const it = test;
export const before = noop;
export const after = noop;
export const beforeEach = noop;
export const afterEach = noop;
export default test;
