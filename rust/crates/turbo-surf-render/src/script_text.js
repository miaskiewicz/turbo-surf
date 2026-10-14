(function () {
  'use strict';
  const g=globalThis,ns=g[Symbol.for('ts')],native=g.__scriptTextNative;
  delete g.__scriptTextNative;
  const apply=Reflect.apply,string=String;
  const check=ns.domAccessor('nodeType',false,'HTML:script');
  const write=ns.domAccessor('textContent',true,'Node');
  Object.defineProperty(g.HTMLScriptElement.prototype,'text',{
    enumerable:true,configurable:true,
    get:ns.makeNativeFunction(function(){return apply(native,this,[]);},'get text',0),
    set:ns.makeNativeFunction(function(value){apply(check,this,[]);if(typeof value==='symbol')throw new TypeError('Cannot convert a Symbol to a string');return apply(write,this,[string(value)]);},'set text',1),
  });
})();
