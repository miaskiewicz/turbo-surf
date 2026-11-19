(async () => {
    const mode = new URL(location.href).searchParams.get('mode');
    const calls = [], results = [], violations = [];
    addEventListener('securitypolicyviolation', event => violations.push({directive:event.effectiveDirective,blockedURI:event.blockedURI,disposition:event.disposition}));
    let current;
    const defaultPolicy = trustedTypes.createPolicy('default', { createScript(input, type, sink) {
      calls.push({ current, input, type, sink });
      if (mode === 'throw') throw new TypeError('authored default-policy rejection');
      if (mode === 'rewrite') return input.replace(/return (\d+)/g, 'return $1 + 100').replace('shadow + 1', 'shadow + 101') + '\n/* rewritten */';
      return input;
    }});
    const brand = trustedTypes.createPolicy('probe', { createScript: input => input });
    const AsyncFunction = Object.getPrototypeOf(async function(){}).constructor;
    const GeneratorFunction = Object.getPrototypeOf(function*(){}).constructor;
    const AsyncGeneratorFunction = Object.getPrototypeOf(async function*(){}).constructor;
    const shadow = 7;
    for (const [name, operation] of [
      ['direct', () => eval('shadow + 1')],
      ['indirect', () => (0, eval)('typeof shadow')],
      ['function', () => new Function('return 9')()],
      ['asyncFunction', () => new AsyncFunction('return 10')()],
      ['generator', () => new GeneratorFunction('yield 11')().next().value],
      ['asyncGenerator', async () => (await new AsyncGeneratorFunction('yield 12')().next()).value],
      ['prefixCollision', () => eval('(function anonymous(\n) {\nreturn 13\n})()')],
      ['brandedDirect', () => eval(brand.createScript('shadow + 2'))],
      ['brandedFunction', () => new Function(brand.createScript('return 14'))()],
      ['objectIdentity', () => { const object = { toString(){ throw new Error('coerced'); } }; return eval(object) === object; }],
      ['boxedIdentity', () => { const object = new String('15'); return eval(object) === object; }],
    ]) {
      current = name;
      const before = calls.length;
      try { results.push({ name, result: await operation(), calls: calls.length - before }); }
      catch (error) { results.push({ name, error: { name: error.name, message: error.message }, calls: calls.length - before }); }
    }
    await new Promise(resolve => setTimeout(resolve, 30));
    return { userAgent: navigator.userAgent, mode, trustedTypesType: typeof trustedTypes, defaultPolicyCreated: !!defaultPolicy, calls, results, violations };
})().then(result => {globalThis.__TT_RESULT=result;}, error => {globalThis.__TT_ERROR={name:error.name,message:error.message};});
