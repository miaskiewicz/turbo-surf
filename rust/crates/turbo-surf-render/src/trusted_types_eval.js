/* eslint-disable no-underscore-dangle, no-unused-vars */
// turbo-surf#121: Chrome 156 routes eval() and new Function() through the
// realm's Trusted Types gate before V8 compiles the source, so a `default`
// policy whose `createScript` returns the input as-is (`s => s`) makes the
// eval succeed, and a rejection throws an EvalError with the specific message
// below.
//
// The exact-engine proxy path uses V8's private source-conversion callback for
// this; the published SDK's V8 lacks that callback, so when
// `require-trusted-types-for 'script'` is enforced we patch globalThis.eval
// and globalThis.Function to run the source through the TT converter (which
// applies the default policy, when one exists) and then compile+run the
// admitted source on the host. Direct-eval semantics are lost — the patched
// eval is a plain global function, so every call is an indirect eval — but
// the common cases (expression evaluation, bootstrap bundles) match Chrome.
//
// The installer is a global so Rust can call it once per context when the
// effective CSP requires Trusted Types. The ops table is captured from the
// bootstrap closure so no page-visible global holds it.
globalThis.__installTtEvalShim = () => {
  if (globalThis.__ttEvalShimInstalled) return;
  Object.defineProperty(globalThis, "__ttEvalShimInstalled", {
    value: true,
    writable: false,
    configurable: false,
    enumerable: false,
  });
  const message =
    "Evaluating a string as JavaScript violates this document's " +
    "Trusted Type assignment requirements.";
  const EvalErrorClass = globalThis.EvalError;
  const FunctionClass = globalThis.Function;
  const document = globalThis.document;
  function convertScript(text, sink) {
    let source;
    try {
      source = ops.op_trusted_type_string(document, text, "TrustedScript", sink, "");
    } catch (_e) {
      throw new EvalErrorClass(message);
    }
    if (source === undefined) throw new EvalErrorClass(message);
    return source;
  }
  function evalShim(input) {
    if (typeof input !== "string") return input;
    return ops.op_run_timer_code(convertScript(input, "eval"));
  }
  Object.defineProperty(evalShim, "name", { value: "eval", configurable: true });
  Object.defineProperty(globalThis, "eval", {
    value: evalShim,
    writable: true,
    configurable: true,
  });
  function FunctionShim(...args) {
    const body = args.length > 0 ? String(args[args.length - 1]) : "";
    const paramParts = [];
    for (let i = 0; i < args.length - 1; i++) paramParts.push(String(args[i]));
    const wrapped = "(function anonymous(" + paramParts.join(",") + "\n) {\n" + body + "\n})";
    return ops.op_run_timer_code(convertScript(wrapped, "Function"));
  }
  FunctionShim.prototype = FunctionClass.prototype;
  Object.defineProperty(FunctionShim, "name", { value: "Function", configurable: true });
  Object.defineProperty(globalThis, "Function", {
    value: FunctionShim,
    writable: true,
    configurable: true,
  });
};
// Response-header CSPs are known before any guest script runs — if this
// realm already requires Trusted Types at bootstrap, patch now so the first
// inline <script> picks up the shim.
if (ops.op_tt_eval_shim_required(globalThis.document)) globalThis.__installTtEvalShim();
