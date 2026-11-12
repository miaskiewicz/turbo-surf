import { createRequire } from "node:module";
import {
  decodeEvaluationValue,
  encodeEvaluationValue,
  encodeEvaluationError,
  decodeEvaluationError,
  evaluationArgument,
} from "./evaluation-codec.mjs";
const native = createRequire(import.meta.url)("../crates/turbo-surf-napi/index.js");
const states = new WeakMap();
const pageOf = (owner) => owner._parentPage ?? owner;
function state(handle, owner) {
  const item = states.get(handle);
  if (!item || item.disposed) throw new Error("JSHandle is disposed");
  const page = pageOf(item.owner);
  if (page._closed || page._session !== item.session)
    throw new Error("JSHandle execution context was destroyed or page is closed");
  item.owner._assertAttached?.();
  if (owner && owner !== item.owner)
    throw new Error("JSHandle belongs to a different page or frame execution context");
  return item;
}
export function hasJSHandles(value, seen = new Set()) {
  if (states.has(value)) return true;
  if (!value || typeof value !== "object" || seen.has(value)) return false;
  seen.add(value);
  return Object.values(value).some((entry) => hasJSHandles(entry, seen));
}
function argumentExpression(value, owner) {
  const references = new Map(),
    declarations = [];
  function visit(value) {
    if (states.has(value)) return `__tsNs.remoteHandleGet(${state(value, owner).id})`;
    if (!value || typeof value !== "object" || value instanceof Date || value instanceof RegExp)
      return evaluationArgument(value);
    if (references.has(value)) return references.get(value);
    const name = `v${references.size}`;
    references.set(value, name);
    declarations.push(`const ${name}=${Array.isArray(value) ? "[]" : "{}"};`);
    for (const [key, entry] of Object.entries(value)) {
      const expression = visit(entry);
      declarations.push(
        `Object.defineProperty(${name},${JSON.stringify(key)},{value:${expression},writable:true,configurable:true,enumerable:true});`,
      );
    }
    return name;
  }
  const result = visit(value);
  return `(()=>{${declarations.join("")}return ${result};})()`;
}
function realm(owner, source) {
  return owner._realmExpression ? owner._realmExpression(source) : source;
}
export async function ensureHandleSession(owner) {
  const page = pageOf(owner);
  if (page._closed) throw new Error("page is closed");
  await owner._ensureLive();
  if (page._session == null) await page._openLiveSession();
}
export async function handleEvaluation(owner, script, arg, retain = false) {
  await ensureHandleSession(owner);
  const page = pageOf(owner);
  if (page._closed || page._session == null)
    throw new Error("page is closed or execution context unavailable");
  const argument = argumentExpression(arg, owner);
  const source =
    typeof script === "function"
      ? `return (${script.toString()})(arg);`
      : `return (${String(script)});`;
  const evaluator = realm(owner, `globalThis.Function('arg',${JSON.stringify(source)})`);
  const window = realm(owner, "globalThis");
  const resultExpression = retain
    ? `({id:__tsNs.remoteHandleRetain(owner,value),kind:value instanceof owner.Element?'element':'value'})`
    : `(${encodeEvaluationValue.toString()})(value)`;
  const body = `(async()=>{try{const owner=${window};const value=await (${evaluator})(${argument});return JSON.stringify({value:${resultExpression}});}catch(error){return JSON.stringify({error:(${encodeEvaluationError.toString()})(error)});}})()`;
  const raw = await native.liveEvalEngineExpression(page._session, body);
  await page._refreshFromSession();
  await page._drainBindings();
  const result = JSON.parse(raw);
  if (result.error) throw decodeEvaluationError(result.error);
  return retain ? createJSHandle(owner, result.value) : decodeEvaluationValue(result.value);
}
export function createJSHandle(owner, descriptor) {
  const handle = descriptor.kind === "element" ? new ElementHandle() : new JSHandle();
  states.set(handle, {
    owner,
    session: pageOf(owner)._session,
    id: descriptor.id,
    disposed: false,
  });
  return handle;
}
export class JSHandle {
  async jsonValue() {
    const item = state(this);
    return handleEvaluation(item.owner, (value) => value, this);
  }
  async evaluate(fn, arg) {
    const item = state(this);
    return handleEvaluation(
      item.owner,
      ({ value, arg, source }) => (0, eval)(`(${source})`)(value, arg),
      {
        value: this,
        arg,
        source: typeof fn === "function" ? fn.toString() : `(value)=>(${String(fn)})`,
      },
    );
  }
  async evaluateHandle(fn, arg) {
    const item = state(this);
    return handleEvaluation(
      item.owner,
      ({ value, arg, source }) => (0, eval)(`(${source})`)(value, arg),
      {
        value: this,
        arg,
        source: typeof fn === "function" ? fn.toString() : `(value)=>(${String(fn)})`,
      },
      true,
    );
  }
  async getProperty(name) {
    return this.evaluateHandle((value, name) => value[name], String(name));
  }
  async getProperties() {
    const names = await this.evaluate((value) => Object.keys(Object(value)));
    const entries = await Promise.all(
      names.map(async (name) => [name, await this.getProperty(name)]),
    );
    return new Map(entries);
  }
  asElement() {
    return null;
  }
  async dispose() {
    const item = states.get(this);
    if (!item || item.disposed) return;
    item.disposed = true;
    const page = pageOf(item.owner);
    if (!page._closed && page._session === item.session)
      await native.liveEvalEngine(
        item.session,
        `__tsNs.remoteHandleDispose(${item.id});globalThis.__RESULT='';`,
      );
  }
  toString() {
    return "JSHandle";
  }
}
export class ElementHandle extends JSHandle {
  asElement() {
    return this;
  }
  getAttribute(name) {
    return this.evaluate((element, name) => element.getAttribute(name), String(name));
  }
  textContent() {
    return this.evaluate((element) => element.textContent);
  }
  innerText() {
    return this.evaluate((element) => element.innerText);
  }
  innerHTML() {
    return this.evaluate((element) => element.innerHTML);
  }
  inputValue() {
    return this.evaluate((element) => element.value);
  }
  isVisible() {
    return this.evaluate((element) => {
      const rect = element.getBoundingClientRect();
      return (
        element.isConnected &&
        getComputedStyle(element).visibility !== "hidden" &&
        rect.width > 0 &&
        rect.height > 0
      );
    });
  }
  isHidden() {
    return this.isVisible().then((value) => !value);
  }
  async _action(name, args) {
    const item = state(this);
    const index = await this.evaluate((element) => {
      if (!element.isConnected) throw new Error("Element is not attached to the DOM");
      return Array.from(document.querySelectorAll("*")).indexOf(element);
    });
    if (index < 0) throw new Error("Element is not attached to the DOM");
    return item.owner
      .locator("*")
      .nth(index)
      [name](...args);
  }
  click(...args) {
    return this._action("click", args);
  }
  fill(...args) {
    return this._action("fill", args);
  }
  focus(...args) {
    return this._action("focus", args);
  }
  press(...args) {
    return this._action("press", args);
  }
  check(...args) {
    return this._action("check", args);
  }
  uncheck(...args) {
    return this._action("uncheck", args);
  }
}
