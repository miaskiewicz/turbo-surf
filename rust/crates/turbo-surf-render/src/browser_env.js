(function(){
  var g = globalThis;
  if (typeof g.navigator === 'undefined') g.navigator = { userAgent: 'turbo-test', platform: 'rust', language: 'en-US', languages: ['en-US'], clipboard: {}, maxTouchPoints: 0 };
  // ---- CSS shorthand expansion ----------------------------------------------------------------
  // getComputedStyle in jsdom derives longhands from shorthands; tests read e.g. marginTop,
  // borderWidth, backgroundColor, rowGap, flexBasis. Expand the common shorthands into longhands so
  // those reads resolve. `emit(prop, value)` writes a single longhand.
  var LINE_STYLES = { none:1,hidden:1,dotted:1,dashed:1,solid:1,double:1,groove:1,ridge:1,inset:1,outset:1 };
  var isLen = function(t){ return /^[\d.]+(px|em|rem|%|pt|vh|vw|vmin|vmax|ch|ex|fr|cm|mm|in)$/.test(t) || t === '0' || /^calc\(/.test(t); };
  var isColor = function(t){ return /^#[0-9a-fA-F]{3,8}$/.test(t) || /^(rgb|rgba|hsl|hsla)\(/i.test(t) || t === 'transparent' || t === 'currentColor' || /^[a-z]+$/i.test(t) && !LINE_STYLES[t] && !isLen(t); };
  // split on top-level whitespace, keeping parenthesized groups (rgb(...), calc(...), url(...)) intact
  var splitTop = function(v){ var out=[], depth=0, cur=''; for (var i=0;i<v.length;i++){ var c=v[i]; if (c==='(') depth++; else if (c===')') depth--; if (/\s/.test(c) && depth===0){ if (cur){ out.push(cur); cur=''; } } else cur+=c; } if (cur) out.push(cur); return out; };
  var box4 = function(p){ return p.length===1 ? [p[0],p[0],p[0],p[0]] : p.length===2 ? [p[0],p[1],p[0],p[1]] : p.length===3 ? [p[0],p[1],p[2],p[1]] : [p[0],p[1],p[2],p[3]]; };
  var expandShorthand = function(name, val, emit){
    var p = splitTop(val.trim());
    // 4-side box shorthands
    if (/^(margin|padding|inset|scroll-margin|scroll-padding)$/.test(name)){
      if (name === 'inset'){ var s4=box4(p); ['top','right','bottom','left'].forEach(function(s,i){ if(s4[i]) emit(s,s4[i]); }); return; }
      var s=box4(p); ['top','right','bottom','left'].forEach(function(side,i){ if(s[i]) emit(name+'-'+side, s[i]); }); return;
    }
    // border-width / -style / -color: also 4-side
    if (/^border-(width|style|color)$/.test(name)){ var which=name.split('-')[1]; var b=box4(p); ['top','right','bottom','left'].forEach(function(side,i){ if(b[i]) emit('border-'+side+'-'+which, b[i]); }); return; }
    // border[-side] / outline: width | style | color (any order)
    if (/^(border(-(top|right|bottom|left))?|outline)$/.test(name)){
      var pre = name;
      p.forEach(function(tok){ if (isLen(tok)||tok==='thin'||tok==='medium'||tok==='thick') emit(pre+'-width', tok); else if (LINE_STYLES[tok]) emit(pre+'-style', tok); else if (isColor(tok)) emit(pre+'-color', tok); });
      return;
    }
    // border-radius: up to 4 corners (ignore the "/" vertical-radii form)
    if (name === 'border-radius'){ var r=val.split('/')[0].trim().split(/\s+/); var c=box4(r); emit('border-top-left-radius',c[0]); emit('border-top-right-radius',c[1]); emit('border-bottom-right-radius',c[2]); emit('border-bottom-left-radius',c[3]); return; }
    // two-axis shorthands
    if (name === 'gap'){ emit('row-gap', p[0]); emit('column-gap', p[1]||p[0]); return; }
    if (name === 'overflow'){ emit('overflow-x', p[0]); emit('overflow-y', p[1]||p[0]); return; }
    if (name === 'place-items' || name === 'place-content' || name === 'place-self'){ var base=name.split('-')[1]; emit('align-'+base, p[0]); emit('justify-'+base, p[1]||p[0]); return; }
    // flex: grow [shrink] [basis]
    if (name === 'flex'){
      if (p.length===1 && p[0]==='none'){ emit('flex-grow','0'); emit('flex-shrink','0'); emit('flex-basis','auto'); return; }
      if (p[0]!=null) emit('flex-grow', p[0]); if (p[1]!=null && /^[\d.]+$/.test(p[1])) emit('flex-shrink', p[1]); var basis = p.filter(function(t){ return isLen(t)||t==='auto'||t==='content'; }); if (basis.length) emit('flex-basis', basis[basis.length-1]); return;
    }
    // font: [style] [variant] [weight] size[/line-height] family...
    if (name === 'font'){
      var fi=0; var weights={normal:1,bold:1,bolder:1,lighter:1,100:1,200:1,300:1,400:1,500:1,600:1,700:1,800:1,900:1};
      while (fi<p.length){ var t=p[fi]; if (t==='italic'||t==='oblique') emit('font-style',t); else if (t==='small-caps') emit('font-variant',t); else if (weights[t]&&!isLen(t)) emit('font-weight',t); else break; fi++; }
      if (fi<p.length){ var sz=p[fi]; var slash=sz.split('/'); emit('font-size', slash[0]); if (slash[1]) emit('line-height', slash[1]); fi++; }
      if (fi<p.length) emit('font-family', p.slice(fi).join(' ')); return;
    }
    // text-decoration: line | style | color
    if (name === 'text-decoration'){
      var lines={none:1,underline:1,overline:1,'line-through':1,blink:1}; var styles={solid:1,double:1,dotted:1,dashed:1,wavy:1};
      p.forEach(function(tok){ if (lines[tok]) emit('text-decoration-line', tok); else if (styles[tok]) emit('text-decoration-style', tok); else if (isColor(tok)) emit('text-decoration-color', tok); }); return;
    }
    // background: extract the color token -> background-color (position/size/repeat are layout-y)
    if (name === 'background'){ for (var bi=0; bi<p.length; bi++){ if (/^#[0-9a-fA-F]{3,8}$/.test(p[bi]) || /^(rgb|rgba|hsl|hsla)\(/i.test(p[bi]) || p[bi]==='transparent'){ emit('background-color', p[bi]); break; } } return; }
  };
  // getComputedStyle reflects the element's inline style object (React writes el.style.X), so
  // jest-dom toHaveStyle (computedStyle[prop] / getPropertyValue(prop)) and toBeVisible (display/
  // visibility/opacity) read real values. No cascade — inline styles only, which covers these tests.
  if (typeof g.getComputedStyle === 'undefined') g.getComputedStyle = function(el){
    var camel = function(p){ return p.replace(/-([a-z])/g, function(_,c){ return c.toUpperCase(); }); };
    var decl = { getPropertyValue: function(p){ var v = this[p]; if (v == null) v = this[camel(p)]; return v == null ? '' : String(v); }, getPropertyPriority: function(){ return ''; }, length: 0 };
    // Minimal cascade: merge declarations from every injected stylesheet rule whose selector matches
    // `el` (emotion's `sx` -> `.css-xxx{...}`), then overlay the element's inline style (highest
    // priority). jsdom-style getComputedStyle so MUI sx values (gradients, etc.) are observable
    // without a full layout engine. Properties are exposed both kebab and camelCase.
    // Normalize comma-spacing ("a,b" -> "a, b"): minified emotion rules drop the space, but jest-dom
    // toHaveStyle's expected value is normalized by the browser's CSS parser (which inserts ", "),
    // so font-family / shorthand lists must match that form.
    // Normalize hex colors to rgb()/rgba() (jsdom's CSS parser does this), matching the form stored
    // by the style proxy, so jest-dom toHaveStyle color assertions compare equal.
    var normColor = function(v){
      var m = /^#([0-9a-fA-F]{3})$/.exec(v); if (m){ var x=m[1]; return 'rgb('+parseInt(x[0]+x[0],16)+', '+parseInt(x[1]+x[1],16)+', '+parseInt(x[2]+x[2],16)+')'; }
      m = /^#([0-9a-fA-F]{6})$/.exec(v); if (m){ var h=m[1]; return 'rgb('+parseInt(h.slice(0,2),16)+', '+parseInt(h.slice(2,4),16)+', '+parseInt(h.slice(4,6),16)+')'; }
      m = /^#([0-9a-fA-F]{8})$/.exec(v); if (m){ var h2=m[1]; return 'rgba('+parseInt(h2.slice(0,2),16)+', '+parseInt(h2.slice(2,4),16)+', '+parseInt(h2.slice(4,6),16)+', '+(Math.round(parseInt(h2.slice(6,8),16)/255*100)/100)+')'; }
      return v;
    };
    // length-valued properties where the computed value is in px (jsdom returns "0px" for a bare 0).
    var LEN_PROP = /(width|height|^top$|^right$|^bottom$|^left$|^inset|margin|padding|gap|^flex-basis$|radius|^font-size$|letter-spacing|word-spacing|text-indent|^outline-offset$|^column-(width|gap)$|size$)/;
    var setProp = function(name, val){ name = String(name).trim(); if (!name) return; if (typeof val === 'string'){ var t = val.trim(); if (t.charAt(0) === '#') val = normColor(t); else if (t === '0' && LEN_PROP.test(name)) val = '0px'; else if (val.indexOf(',') >= 0) val = val.replace(/\s*,\s*/g, ', '); } decl[name] = val; decl[camel(name)] = val;
      if (typeof val === 'string') expandShorthand(name, val, setProp);
    };
    try {
      var sheets = el && el.ownerDocument ? (el.ownerDocument.styleSheets || []) : [];
      for (var si=0; si<sheets.length; si++) {
        var rules = sheets[si].cssRules || [];
        for (var ri=0; ri<rules.length; ri++) {
          var txt = String(rules[ri].cssText || ''); var br = txt.indexOf('{'); if (br < 0) continue;
          var sel = txt.slice(0, br).trim(); var body = txt.slice(br+1, txt.lastIndexOf('}'));
          var matched = false;
          var parts = sel.split(',');
          for (var pi=0; pi<parts.length; pi++){ var s = parts[pi].trim(); if (!s) continue; try { if (el.matches && el.matches(s)) { matched = true; break; } } catch(e){} }
          if (!matched) continue;
          var ds = body.split(';');
          for (var di=0; di<ds.length; di++){ var c = ds[di].indexOf(':'); if (c<0) continue; setProp(ds[di].slice(0,c), ds[di].slice(c+1).trim()); }
        }
      }
    } catch(e){}
    // overlay inline style (own props React/components set: background, height, display, ...)
    var st = el && el.style;
    if (st) { for (var k in st) { if (k.indexOf('__') !== 0 && Object.prototype.hasOwnProperty.call(st, k) && typeof st[k] !== 'function') setProp(k, st[k]); } }
    if (decl.display == null) decl.display = '';
    // `visibility` is INHERITED: a child with no explicit visibility takes the parent's computed
    // value. testing-library / dom-accessibility-api rely on this — they treat an element as
    // inaccessible when getComputedStyle(el).visibility === 'hidden', expecting a hidden ancestor to
    // have propagated down. The JS cascade above only sees this element's own rules, so we defer to
    // rtdom's native cascade (cascade::computed_style) which resolves inheritance. display is NOT
    // inherited, so it is left as the element's own value.
    if ((decl.visibility == null || decl.visibility === '') && el && typeof el.__cascadeProp === 'function') {
      try { decl.visibility = el.__cascadeProp('visibility') || ''; } catch(e){}
    }
    if (decl.visibility == null) decl.visibility = '';
    if (decl.opacity == null) decl.opacity = '';
    // Used-value resolution for percentage width/height. A no-layout env returns a raw "50%"
    // verbatim, but a containing-block percentage has a well-defined used value in normal flow
    // (pct * the containing block's content width/height), which a real browser reports in px.
    // Resolve against the nearest ancestor with a px (or, for width, auto=fill) measure so
    // getComputedStyle(child).width matches a browser without a full layout engine.
    var usedPx = function(node, axis, depth){
      if (!node || depth > 32) return null;
      var cs; try { cs = (node === el) ? decl : g.getComputedStyle(node); } catch(e){ return null; }
      var raw = cs && (axis === 'width' ? cs.width : cs.height); raw = raw == null ? '' : String(raw);
      var mpx = /^(-?\d+(?:\.\d+)?)px$/.exec(raw); if (mpx) return parseFloat(mpx[1]);
      var parent = node.parentElement || (node.parentNode && node.parentNode.nodeType === 1 ? node.parentNode : null);
      var mpc = /^(-?\d+(?:\.\d+)?)%$/.exec(raw);
      if (mpc) { var base = usedPx(parent, axis, depth + 1); return base == null ? null : (parseFloat(mpc[1]) / 100) * base; }
      // width:auto in normal flow fills the containing block (block box); height:auto is content-driven (unknown here).
      if ((raw === '' || raw === 'auto') && axis === 'width') return usedPx(parent, axis, depth + 1);
      return null;
    };
    try {
      ['width', 'height'].forEach(function(axis){
        var raw = decl[axis];
        if (typeof raw === 'string' && /^-?\d+(?:\.\d+)?%$/.test(raw.trim())) {
          var px = usedPx(el, axis, 0);
          if (px != null && isFinite(px)) setProp(axis, (Math.round(px * 1000) / 1000) + 'px');
        }
      });
    } catch(e){}
    return decl;
  };
  if (typeof g.requestAnimationFrame === 'undefined') g.requestAnimationFrame = function(cb){ return setTimeout(function(){ cb(Date.now()); }, 0); };
  if (typeof g.cancelAnimationFrame === 'undefined') g.cancelAnimationFrame = function(id){ clearTimeout(id); };
  if (typeof g.matchMedia === 'undefined') g.matchMedia = function(q){ return { matches:false, media:q, addListener:function(){}, removeListener:function(){}, addEventListener:function(){}, removeEventListener:function(){}, dispatchEvent:function(){return false;} }; };
  if (typeof g.scrollTo === 'undefined') g.scrollTo = function(){};
  // `new Image(w,h)` -> an <img> element (HTMLImageElement). Setting `.src` resolves `onload`
  // asynchronously (no real network/decoding) so avatar/image-load effects fire.
  if (typeof g.Image === 'undefined') {
    g.Image = function(w, h){
      var img = g.document.createElement('img');
      if (w != null) img.width = w; if (h != null) img.height = h;
      var _src = '';
      try { Object.defineProperty(img, 'src', { configurable: true, get: function(){ return _src; }, set: function(v){ _src = v == null ? '' : String(v); this.setAttribute('src', _src); var self = this; if (_src) setTimeout(function(){ if (typeof self.onload === 'function') self.onload({ type: 'load', target: self }); }, 0); } }); } catch(e){}
      return img;
    };
  }
  if (typeof g.location === 'undefined' || g.location == null) {
    g.location = { href: 'http://localhost/', origin: 'http://localhost', protocol: 'http:', host: 'localhost', hostname: 'localhost', port: '', pathname: '/', search: '', hash: '', assign: function(){}, replace: function(){}, reload: function(){}, toString: function(){ return this.href; } };
  }
  // DOM interface constructors (for `instanceof` / global presence). Stubs; identity not enforced.
  // Real Event base class (dispatch in browser_env.rs reads type/bubbles/defaultPrevented/__stop*).
  if (!g.__ttEvent) {
    function Event(type, init){ init = init || {}; this.type = type; this.bubbles = !!init.bubbles; this.cancelable = !!init.cancelable; this.composed = !!init.composed; this.defaultPrevented = false; this.target = null; this.currentTarget = null; this.__stop = false; this.__stopImmediate = false; this.eventPhase = 0; this.timeStamp = Date.now(); this.isTrusted = false; this.detail = init.detail; }
    Event.prototype.preventDefault = function(){ if (this.cancelable) this.defaultPrevented = true; };
    Event.prototype.stopPropagation = function(){ this.__stop = true; };
    Event.prototype.stopImmediatePropagation = function(){ this.__stop = true; this.__stopImmediate = true; };
    Event.prototype.initEvent = function(type, bubbles, cancelable){ this.type = type; this.bubbles = !!bubbles; this.cancelable = !!cancelable; };
    Event.NONE = 0; Event.CAPTURING_PHASE = 1; Event.AT_TARGET = 2; Event.BUBBLING_PHASE = 3;
    g.Event = Event;
    function mkSub(extra){ return function(type, init){ init = init || {}; Event.call(this, type, init); if (extra) extra(this, init); for (var k in init){ if (!(k in this)) this[k] = init[k]; } }; }
    function sub(name, extra){ var C = mkSub(extra); C.prototype = Object.create(Event.prototype); C.prototype.constructor = C; g[name] = C; }
    var keyExtra = function(self, init){ self.key = init.key||''; self.code = init.code||''; self.keyCode = init.keyCode||0; self.which = init.which||init.keyCode||0; self.altKey=!!init.altKey; self.ctrlKey=!!init.ctrlKey; self.metaKey=!!init.metaKey; self.shiftKey=!!init.shiftKey; self.repeat=!!init.repeat; };
    var mouseExtra = function(self, init){ self.button=init.button||0; self.buttons=init.buttons||0; self.clientX=init.clientX||0; self.clientY=init.clientY||0; self.pageX=init.pageX||0; self.pageY=init.pageY||0; self.altKey=!!init.altKey; self.ctrlKey=!!init.ctrlKey; self.metaKey=!!init.metaKey; self.shiftKey=!!init.shiftKey; self.relatedTarget=init.relatedTarget||null; };
    sub('CustomEvent', function(self, init){ self.detail = init.detail; });
    sub('UIEvent'); sub('MouseEvent', mouseExtra); sub('PointerEvent', mouseExtra); sub('KeyboardEvent', keyExtra);
    sub('InputEvent', function(self, init){ self.data = init.data; self.inputType = init.inputType||''; });
    sub('FocusEvent', function(self, init){ self.relatedTarget = init.relatedTarget||null; });
    sub('CompositionEvent'); sub('WheelEvent', mouseExtra); sub('DragEvent', mouseExtra); sub('TouchEvent'); sub('ClipboardEvent');
    g.document.createEvent = function(){ return new Event('', {}); };
    g.__ttEvent = true;
  }
  var ctors = ['Node','Element','HTMLElement','HTMLDivElement','HTMLInputElement','HTMLButtonElement','HTMLAnchorElement','HTMLSelectElement','HTMLTextAreaElement','HTMLFormElement','HTMLImageElement','HTMLLabelElement','HTMLOptionElement','HTMLUListElement','HTMLLIElement','HTMLSpanElement','HTMLParagraphElement','HTMLHeadingElement','HTMLTableElement','HTMLIFrameElement','HTMLCanvasElement','HTMLStyleElement','HTMLScriptElement','HTMLDocument','Document','DocumentFragment','ShadowRoot','Text','Comment','SVGElement','SVGSVGElement','DOMParser','EventTarget','AbortController','AbortSignal','DOMException',
    'UIEvent','MouseEvent','KeyboardEvent','FocusEvent','InputEvent','TouchEvent','PointerEvent','WheelEvent','DragEvent','ClipboardEvent','AnimationEvent','TransitionEvent','MessageEvent','ProgressEvent','CompositionEvent','PopStateEvent','HashChangeEvent','StorageEvent','ErrorEvent','CloseEvent',
    // DOM node/collection/CSS interfaces app bundles do `instanceof` against (an
    // undefined RHS throws "not an object" and aborts the chunk — a single missing
    // `Response`/`NodeList` cascades through webpack init and kills React hydration).
    'CharacterData','Attr','CDATASection','ProcessingInstruction','NodeList','HTMLCollection',
    'DOMTokenList','NamedNodeMap','CSSStyleDeclaration','CSSRule','StyleSheet','MediaList',
    'DOMStringMap','MutationRecord','MediaQueryList','DOMRect','DOMRectReadOnly','DOMPoint'];
  ctors.forEach(function(n){ if (typeof g[n] === "undefined") { var f = function(){}; f.prototype = {}; try { Object.defineProperty(f, "name", { value: n, configurable: true }); } catch(e){} g[n] = f; } });
  // EventTarget must be a REAL subclassable base. Apps + libraries write
  // `class Foo extends EventTarget { … this.addEventListener(…) }` (event buses, modal
  // managers, state stores — Nike's global-nav modal manager does exactly this); with an
  // empty `EventTarget.prototype` the instance's `addEventListener` is undefined and the
  // call throws. An uncaught throw mid-hydration makes React discard the SSR tree and
  // client-render the whole root (#425), blanking server-rendered content. Give the
  // prototype a working listener registry so subclass instances behave as event targets.
  (function(){
    var ET = g.EventTarget;
    if (ET && ET.prototype && typeof ET.prototype.addEventListener !== "function") {
      ET.prototype.addEventListener = function(type, cb){
        if (!cb || (typeof cb !== "function" && typeof cb.handleEvent !== "function")) return;
        var m = this.__etl || (this.__etl = {}); (m[type] = m[type] || []).push(cb);
      };
      ET.prototype.removeEventListener = function(type, cb){
        var m = this.__etl; if (!m || !m[type]) return;
        var i = m[type].indexOf(cb); if (i >= 0) m[type].splice(i, 1);
      };
      ET.prototype.dispatchEvent = function(ev){
        var m = this.__etl, type = ev && ev.type, list = m && type && m[type];
        if (list) { try { if (!ev.target) ev.target = this; ev.currentTarget = this; } catch(e){}
          for (var i = 0; i < list.length; i++){ var cb = list[i];
            try { typeof cb === "function" ? cb.call(this, ev) : cb.handleEvent(ev); } catch(e){} } }
        return !(ev && ev.defaultPrevented);
      };
    }
  })();
  // make `node instanceof HTMLElement/Element/Node/...` work for native DOM nodes (jest-dom's
  // checkHtmlElement + many libs rely on it) via Symbol.hasInstance keyed on nodeType.
  function iface(name, pred){ var f = g[name] || function(){}; try { Object.defineProperty(f, Symbol.hasInstance, { configurable: true, value: pred }); } catch(e){} g[name] = f; }
  var isNode = function(o){ return o != null && typeof o === 'object' && typeof o.nodeType === 'number'; };
  iface('Node', isNode);
  iface('Element', function(o){ return isNode(o) && o.nodeType === 1; });
  iface('HTMLElement', function(o){ return isNode(o) && o.nodeType === 1; });
  iface('SVGElement', function(o){ return isNode(o) && o.nodeType === 1 && String(o.namespaceURI||'').indexOf('svg') >= 0; });
  iface('Text', function(o){ return isNode(o) && o.nodeType === 3; });
  iface('Comment', function(o){ return isNode(o) && o.nodeType === 8; });
  iface('DocumentFragment', function(o){ return isNode(o) && o.nodeType === 11; });
  iface('HTMLInputElement', function(o){ return isNode(o) && o.nodeType === 1 && String(o.tagName).toUpperCase() === 'INPUT'; });
  iface('HTMLTextAreaElement', function(o){ return isNode(o) && o.nodeType === 1 && String(o.tagName).toUpperCase() === 'TEXTAREA'; });
  iface('HTMLSelectElement', function(o){ return isNode(o) && o.nodeType === 1 && String(o.tagName).toUpperCase() === 'SELECT'; });
  // Node subtypes keyed on nodeType (CharacterData is the Text/Comment/CDATA base).
  iface('CharacterData', function(o){ return isNode(o) && (o.nodeType === 3 || o.nodeType === 4 || o.nodeType === 8); });
  iface('Attr', function(o){ return isNode(o) && o.nodeType === 2; });
  iface('CDATASection', function(o){ return isNode(o) && o.nodeType === 4; });
  iface('ProcessingInstruction', function(o){ return isNode(o) && o.nodeType === 7; });
  // CSSStyleDeclaration: what el.style / getComputedStyle() return (has getPropertyValue).
  iface('CSSStyleDeclaration', function(o){ return o != null && typeof o === 'object' && typeof o.getPropertyValue === 'function'; });
  // Live-ish collections: array-like with length + item(). NodeList also allows forEach.
  iface('HTMLCollection', function(o){ return o != null && typeof o === 'object' && typeof o.length === 'number' && typeof o.item === 'function'; });
  iface('NodeList', function(o){ return o != null && typeof o === 'object' && typeof o.length === 'number' && (typeof o.item === 'function' || typeof o.forEach === 'function'); });
  iface('DOMTokenList', function(o){ return o != null && typeof o === 'object' && typeof o.length === 'number' && typeof o.toggle === 'function'; });
  // Audio(src) — a real HTMLAudioElement built through the DOM (so it participates in
  // the tree + carries the media API stubs below), matching `new Audio(url)`.
  if (typeof g.Audio === 'undefined') {
    g.Audio = function Audio(src){
      var el = d.createElement('audio');
      if (src != null) { try { el.src = String(src); } catch(e){} }
      return el;
    };
    try { g.Audio.prototype = (g.HTMLAudioElement && g.HTMLAudioElement.prototype) || {}; } catch(e){}
  }
  // Media-element methods on the shared element prototype (headless has no audio/video
  // pipeline; these no-op so `el.play()`/`.load()` don't throw and `.play()` still
  // returns a promise as the spec requires). `canPlayType` reports no support.
  (function(){
    var mp = (g.HTMLElement && g.HTMLElement.prototype) || {};
    if (typeof mp.play !== 'function') mp.play = function(){ return Promise.resolve(); };
    if (typeof mp.pause !== 'function') mp.pause = function(){};
    if (typeof mp.load !== 'function') mp.load = function(){};
    if (typeof mp.canPlayType !== 'function') mp.canPlayType = function(){ return ''; };
  })();
  // Worker — a real class with the full interface. A single deno_core isolate has no
  // second thread, so it doesn't spawn one (the worker body doesn't run); it queues
  // postMessage + exposes onmessage/onerror + terminate so apps that construct a
  // Worker (analytics, off-main-thread hashing) don't throw and degrade gracefully.
  if (typeof g.Worker === 'undefined') {
    g.Worker = function Worker(url){ this.url = String(url || ''); this.onmessage = null; this.onmessageerror = null; this.onerror = null; this._l = {}; };
    g.Worker.prototype.postMessage = function(){};
    g.Worker.prototype.terminate = function(){};
    g.Worker.prototype.addEventListener = function(t, f){ (this._l[t] = this._l[t] || []).push(f); };
    g.Worker.prototype.removeEventListener = function(t, f){ var a = this._l[t]; if (a){ var i = a.indexOf(f); if (i >= 0) a.splice(i, 1); } };
    g.Worker.prototype.dispatchEvent = function(){ return true; };
    if (typeof g.SharedWorker === 'undefined') { g.SharedWorker = function SharedWorker(url){ this.url = String(url || ''); this.port = { postMessage: function(){}, start: function(){}, close: function(){}, addEventListener: function(){}, removeEventListener: function(){}, onmessage: null }; }; }
  }
  // Extra HTML*Element constructors the base ctor list omits. App bundles reference these for
  // feature-detect / instanceof / subclassing (MUI's Dialog touches HTMLDialogElement); an
  // undefined reference aborts the chunk mid-hydration, blanking the tree. Single-tag elements get
  // a tag-keyed `instanceof`; abstract / multi-tag interfaces fall back to a generic element check.
  var htmlByTag = {
    DIALOG:'HTMLDialogElement', DATALIST:'HTMLDataListElement', FIELDSET:'HTMLFieldSetElement',
    LEGEND:'HTMLLegendElement', OL:'HTMLOListElement', DL:'HTMLDListElement', PRE:'HTMLPreElement',
    TR:'HTMLTableRowElement', COL:'HTMLTableColElement', CAPTION:'HTMLTableCaptionElement',
    PROGRESS:'HTMLProgressElement', METER:'HTMLMeterElement', DETAILS:'HTMLDetailsElement',
    PICTURE:'HTMLPictureElement', SOURCE:'HTMLSourceElement', VIDEO:'HTMLVideoElement',
    AUDIO:'HTMLAudioElement', TEMPLATE:'HTMLTemplateElement', SLOT:'HTMLSlotElement',
    BODY:'HTMLBodyElement', HTML:'HTMLHtmlElement', HEAD:'HTMLHeadElement', META:'HTMLMetaElement',
    LINK:'HTMLLinkElement', TITLE:'HTMLTitleElement', BASE:'HTMLBaseElement', BR:'HTMLBRElement',
    HR:'HTMLHRElement', OPTGROUP:'HTMLOptGroupElement', MAP:'HTMLMapElement', AREA:'HTMLAreaElement',
    OBJECT:'HTMLObjectElement', EMBED:'HTMLEmbedElement', OUTPUT:'HTMLOutputElement',
    MENU:'HTMLMenuElement', DATA:'HTMLDataElement', TIME:'HTMLTimeElement',
  };
  // abstract base / multi-tag interfaces (no single tag → generic nodeType-1 instanceof).
  var htmlAbstract = ['HTMLMediaElement','HTMLTableCellElement','HTMLTableSectionElement','HTMLQuoteElement','HTMLUnknownElement'];
  (function(){
    var baseProto = (g.HTMLElement && g.HTMLElement.prototype) || {};
    var mk = function(name){ var f = g[name]; if (typeof f !== 'function') { f = function(){}; f.prototype = Object.create(baseProto); try { Object.defineProperty(f, 'name', { value: name, configurable: true }); } catch(e){} g[name] = f; } return f; };
    Object.keys(htmlByTag).forEach(function(tag){ var name = htmlByTag[tag]; mk(name); iface(name, (function(t){ return function(o){ return isNode(o) && o.nodeType === 1 && String(o.tagName).toUpperCase() === t; }; })(tag)); });
    htmlAbstract.forEach(function(name){ mk(name); iface(name, function(o){ return isNode(o) && o.nodeType === 1; }); });
  })();
  // window-level event listeners — a real registry so window.addEventListener('keydown', …) +
  // window.dispatchEvent(new KeyboardEvent(...)) work (e.g. global keyboard shortcuts).
  if (!g.__winListeners) {
    var winL = g.__winListeners = {};
    g.addEventListener = function(type, fn){ if (typeof fn !== 'function') return; (winL[type] = winL[type] || []).push(fn); };
    g.removeEventListener = function(type, fn){ var a = winL[type]; if (a){ var i = a.indexOf(fn); if (i >= 0) a.splice(i, 1); } };
    g.dispatchEvent = function(ev){ if (!ev) return true; if (ev.target == null) { try { ev.target = g; } catch(e){} } var a = winL[ev.type]; if (a) a.slice().forEach(function(fn){ try { fn.call(g, ev); } catch(e){} }); return !ev.defaultPrevented; };
  }
  // document extras (pure-JS shims over the native tree).
  var d = g.document;
  d.defaultView = g;
  // Constructable stylesheets — emotion/MUI do `new CSSStyleSheet()` then
  // `document.adoptedStyleSheets = [...document.adoptedStyleSheets, sheet]`. There is
  // no layout engine here, so the rule list is an inert store that satisfies the API;
  // a missing `CSSStyleSheet` reference aborts the chunk mid-hydration, blanking the tree.
  if (typeof g.CSSStyleSheet === 'undefined') {
    g.CSSStyleSheet = function CSSStyleSheet(){ this.cssRules = []; this.rules = this.cssRules; };
    g.CSSStyleSheet.prototype.insertRule = function(rule, index){
      var i = (index == null) ? this.cssRules.length : index;
      this.cssRules.splice(i, 0, { cssText: String(rule) });
      return i;
    };
    g.CSSStyleSheet.prototype.deleteRule = function(index){ this.cssRules.splice(index, 1); };
    g.CSSStyleSheet.prototype.replace = function(text){ this.cssRules = [{ cssText: String(text) }]; return Promise.resolve(this); };
    g.CSSStyleSheet.prototype.replaceSync = function(text){ this.cssRules = [{ cssText: String(text) }]; };
  }
  // adoptedStyleSheets — must be a real, settable ARRAY (a non-iterable getter or
  // absent prop both break the read-modify-write spread above).
  if (!Array.isArray(d.adoptedStyleSheets)) {
    try { d.adoptedStyleSheets = []; } catch (e) {}
  }
  // document.title reflects the <title> element's text (jsdom semantics). Ensure a <title> exists in
  // <head> so code that does querySelector('title').textContent = … (usePageMetadata) has a target,
  // and reading document.title returns it.
  {
    var ensureTitle = function(){
      var el = d.querySelector && d.querySelector('title');
      if (!el) { el = d.createElement('title'); var head = (d.head) || (d.querySelector && d.querySelector('head')) || d.documentElement || d.body; if (head && head.appendChild) head.appendChild(el); }
      return el;
    };
    try { ensureTitle(); } catch(e){}
    try { Object.defineProperty(d, 'title', { configurable: true,
      get: function(){ var el = d.querySelector && d.querySelector('title'); return el ? (el.textContent || '') : ''; },
      set: function(v){ var el = ensureTitle(); if (el) el.textContent = String(v == null ? '' : v); } }); } catch(e){}
  }
  // document.cookie jar. Defined on the document PROTOTYPE (not the instance) — code reads
  // `Object.getOwnPropertyDescriptor(Object.getPrototypeOf(document), 'cookie')` to wrap it. Setter
  // parses "k=v; attrs"; max-age<=0 deletes. Getter serializes the live pairs.
  (function(){
    var proto = Object.getPrototypeOf(d) || d;
    if (!Object.getOwnPropertyDescriptor(proto, 'cookie')) {
      var jar = {};
      try { Object.defineProperty(proto, 'cookie', {
        configurable: true,
        get: function(){ var out = []; for (var k in jar) out.push(k + '=' + jar[k]); return out.join('; '); },
        set: function(str){
          str = String(str == null ? '' : str);
          var parts = str.split(';'); var first = parts[0] || ''; var eq = first.indexOf('=');
          if (eq < 0) return; var name = first.slice(0, eq).trim(); var val = first.slice(eq + 1).trim();
          var del = false;
          for (var i = 1; i < parts.length; i++){ var p = parts[i].trim(); var m = /^max-age\s*=\s*(-?\d+)/i.exec(p); if (m && parseInt(m[1], 10) <= 0) del = true; if (/^expires\s*=/i.test(p) && /1970/.test(p)) del = true; }
          if (del) { delete jar[name]; } else { jar[name] = val; }
        }
      }); } catch(e){}
    }
  })();
  // React's isEventSupported('input') checks `'oninput' in document`; make it true (+ keep
  // documentMode ABSENT) so React uses the modern input/change path, not the IE change polyfill —
  // otherwise fireEvent.change never fires onChange.
  d.oninput = null; d.onchange = null; d.onclick = null; d.onkeydown = null; d.onkeyup = null;
  if (!d.head) { try { d.head = d.createElement('head'); if (d.documentElement) d.documentElement.appendChild(d.head); } catch(e){} }
  // NOTE: document.createElementNS uses the native binding (namespace-aware: SVG/MathML elements keep
  // their namespace + case-preserved attributes like viewBox). Previously aliased to createElement.
  d.createDocumentFragment = function(){ return d.createElement('#document-fragment'); };
  // NOTE: document.addEventListener/removeEventListener/dispatchEvent use the native binding (the
  // document is a wrapped node) so document-level listeners — click-outside handlers, global
  // keydown — register and fire via the bubble path. (Previously stubbed to no-ops.)
  // CSSOM shim (<style>.sheet) + form-control value/checked. value/checked are defined BOTH as an
  // OWN accessor on each control element (testing-library's setNativeValue reads
  // getOwnPropertyDescriptor(element)) AND on the interface .prototype (React's value-tracker reads
  // node.constructor.prototype). No setPrototypeOf — avoids touching the native proto chain.
  (function(){
    var sheets = [];
    var mkSheet = function(el){ var rules = []; return { ownerNode: el, cssRules: rules, get rules(){ return rules; },
      insertRule: function(rule, index){ var i = index == null ? rules.length : index; rules.splice(i, 0, { cssText: String(rule), selectorText: '' });
        // Also reflect into the <style> element's text so code reading style.textContent (emotion's
        // non-speedy form, which some tests assume) sees the inserted CSS.
        try { el.textContent = (el.textContent || '') + String(rule); } catch(e){}
        return i; },
      deleteRule: function(i){ rules.splice(i, 1); try { el.textContent = rules.map(function(r){ return r.cssText; }).join(''); } catch(e){} },
      replaceSync: function(){}, replace: function(){ return Promise.resolve(); } }; };
    var orig = d.createElement.bind(d);
    // value/checked live ONLY on the interface .prototype (NOT own — React's value-tracker bails on
    // node.hasOwnProperty('value')). Each control's actual proto is set to that interface prototype
    // so getPrototypeOf(el) === el.constructor.prototype has the descriptor (React + testing-library).
    // A `date`/`time` <input> only retains a value the browser can parse — an invalid/partial string
    // sets value to ''. userEvent.type relies on this (isValidDateOrTimeValue clones, assigns, and
    // checks the value stuck) to commit the typed value + fire onChange only once a full valid date
    // is entered. Without it, partials commit and the controlled reset mangles the result.
    var validForType = function(el, v){
      var t = (el.getAttribute('type') || 'text').toLowerCase();
      if (t === 'date') return /^\d{4}-\d{2}-\d{2}$/.test(v) ? v : '';
      if (t === 'time') return /^\d{2}:\d{2}(:\d{2})?$/.test(v) ? v : '';
      if (t === 'month') return /^\d{4}-\d{2}$/.test(v) ? v : '';
      if (t === 'datetime-local') return /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}/.test(v) ? v : '';
      // A number input only retains a valid floating-point number; an invalid/partial value ('.', '-')
      // becomes '' (HTML value-sanitization), which is what lets components see the empty-string clear.
      if (t === 'number') return (v === '' || /^[+-]?(\d+\.?\d*|\.\d+)([eE][+-]?\d+)?$/.test(v)) ? v : '';
      return v;
    };
    // The `value` IDL accessor, backed by the `value` content attribute (defaultValue==value here,
    // which suffices for getByDisplayValue/[value]); the value-change tracker (attachValueTracker)
    // layers the dirty-since-last-seen signal on top for framework change detection.
    var valDesc = { configurable: true, get: function(){ var v = this.getAttribute('value'); return v == null ? '' : v; }, set: function(v){ this.setAttribute('value', validForType(this, v == null ? '' : String(v))); } };
    // Reflect the live `checked` property to the `checked` content attribute so rtdom's `:checked`
    // pseudo (which matches on the attribute) finds controlled checkboxes/radios React toggles.
    var checkedDesc = { configurable: true, get: function(){ return this.__checked === undefined ? this.hasAttribute('checked') : !!this.__checked; }, set: function(v){ v = !!v; this.__checked = v; try { if (v) this.setAttribute('checked', ''); else this.removeAttribute('checked'); } catch(e){} } };
    // `type` defaults to 'text' for <input> — React's isTextInputElement keys on it to pick the
    // input/change handling path; undefined would route changes to the select/checkbox path.
    var mkTypeDesc = function(def){ return { configurable: true, get: function(){ return (this.getAttribute('type') || def).toLowerCase(); }, set: function(v){ this.setAttribute('type', v); } }; };
    // `form` → the nearest ancestor <form> (userEvent fires submit on a submit-button's form).
    var formDesc = { configurable: true, get: function(){ return this.closest ? this.closest('form') : null; } };
    // text-cursor selection (userEvent.type inserts at selectionStart..selectionEnd; undefined -> NaN
    // -> nothing gets typed). Default the cursor to the end of the value.
    var selStartDesc = { configurable: true, get: function(){ return this.__selStart == null ? String(this.value||'').length : this.__selStart; }, set: function(v){ this.__selStart = v; } };
    var selEndDesc = { configurable: true, get: function(){ return this.__selEnd == null ? String(this.value||'').length : this.__selEnd; }, set: function(v){ this.__selEnd = v; } };
    var selDirDesc = { configurable: true, get: function(){ return this.__selDir || 'none'; }, set: function(v){ this.__selDir = v; } };
    var baseProto = Object.getPrototypeOf(orig('span'));
    // Reflected IDL attributes on every element: setting el.src / el.href (script/img/link/a — a DOM
    // property) must show as the content attribute so attribute selectors like script[src*="maps"]
    // and getAttribute('src') see it. (Image() defines its own src with onload; that shadows this.)
    // Reflected string IDL attributes on every element. `rel` is load-bearing: React's
    // clearContainerSparingly does `node.rel.toLowerCase()` over a container's <link> children during
    // hydration — undefined there aborts the whole app mount. `as`/`media`/`type` ride the same
    // pattern (<link rel=preload as=script media=…>, <script type=…>). For inputs `type` is shadowed
    // per-instance by the text-defaulting control descriptor (applyControlProto), so this is the
    // fallback for non-control elements. (Image() defines its own src with onload; that shadows.)
    ['src','href','rel','as','media','type'].forEach(function(a){ if (!Object.getOwnPropertyDescriptor(baseProto, a)) { try { Object.defineProperty(baseProto, a, { configurable: true, get: function(){ return this.getAttribute(a) || ''; }, set: function(v){ this.setAttribute(a, v == null ? '' : String(v)); } }); } catch(e){} } });
    // CharacterData mutation methods (insertData/deleteData/appendData/replaceData/substringData/
    // splitText) are now native rtdom Tree ops bound on every node — see browser_env.rs.
    // input.valueAsNumber — the numeric view of `value`, NaN when non-numeric/empty. MUI Slider's
    // hidden range input reads event.target.valueAsNumber in its change handler to derive the new
    // value; without it the value came through as null. (number/range inputs only; others give NaN.)
    if (!Object.getOwnPropertyDescriptor(baseProto, 'valueAsNumber')) { try { Object.defineProperty(baseProto, 'valueAsNumber', { configurable: true, get: function(){ var v = this.value; if (v == null || v === '') return NaN; var n = Number(v); return isNaN(n) ? NaN : n; }, set: function(n){ this.value = (n == null || isNaN(n)) ? '' : String(n); } }); } catch(e){} }
    // Element.getClientRects() — no layout engine, so mirror getBoundingClientRect's zero rect as a
    // single-entry DOMRectList-ish (with .item). Libs do getClientRects()[0] / .length; [] would NPE.
    if (!baseProto.getClientRects) baseProto.getClientRects = function(){ var r = this.getBoundingClientRect ? this.getBoundingClientRect() : { x:0,y:0,top:0,left:0,right:0,bottom:0,width:0,height:0 }; var list = [r]; list.item = function(i){ return list[i] || null; }; return list; };
    // isEqualNode — React 18 hydration calls it on EVERY node to reconcile the server
    // markup with the client render; without it every comparison throws
    // "e.isEqualNode is not a function", React reports a hydration mismatch (#418) and
    // discards the SSR tree → the whole app falls back to a blank client render. Real
    // spec-shaped deep structural equality (same type/name, same attributes, same
    // character data, recursively-equal children). Recurses via a helper (not
    // child.isEqualNode) so it works even on node types that miss the method.
    if (!baseProto.isEqualNode) {
      var __nodeEq = function(a, b){
        if (a === b) return true;
        if (!a || !b) return false;
        if (a.nodeType !== b.nodeType || a.nodeName !== b.nodeName) return false;
        if (a.nodeType === 1) {
          var aa = a.attributes || [], ba = b.attributes || [];
          if ((aa.length || 0) !== (ba.length || 0)) return false;
          for (var i = 0; i < aa.length; i++) {
            var at = aa[i], nm = at && (at.name != null ? at.name : at.nodeName);
            if (nm == null) continue;
            if (!b.getAttribute || b.getAttribute(nm) !== (at.value != null ? at.value : at.nodeValue)) return false;
          }
        } else if ((a.nodeValue || '') !== (b.nodeValue || '')) {
          return false;
        }
        var ac = a.childNodes || [], bc = b.childNodes || [];
        if ((ac.length || 0) !== (bc.length || 0)) return false;
        for (var j = 0; j < ac.length; j++) { if (!__nodeEq(ac[j], bc[j])) return false; }
        return true;
      };
      baseProto.isEqualNode = function(other){ return __nodeEq(this, other); };
      if (!baseProto.isSameNode) baseProto.isSameNode = function(other){ return this === other; };
    }
    // getElementsByTagName / getElementsByClassName / getElementsByName over querySelectorAll. The
    // native binding ships querySelector(All) only; libs (jQuery's load-time support probe does
    // el.getElementsByTagName('input')[0].checked) need these. Add to the shared element prototype
    // (guarded so a native impl wins if ever added).
    // Walk descendants via `children` (works on a DETACHED subtree, unlike querySelectorAll which
    // matches only connected nodes — same reason select.options walks children).
    var geWalk = function(node, match){ var out = []; (function visit(n){ var kids = n.children || []; for (var i=0;i<kids.length;i++){ if (match(kids[i])) out.push(kids[i]); visit(kids[i]); } })(node); out.item = function(i){ return out[i] || null; }; out.namedItem = function(nm){ for (var i=0;i<out.length;i++){ var e=out[i]; if (e && (e.id === nm || (e.getAttribute && e.getAttribute('name') === nm))) return e; } return null; }; return out; };
    var geByTag = function(t){ var want = String(t).toUpperCase(); return geWalk(this, function(e){ return want === '*' || String(e.tagName).toUpperCase() === want; }); };
    var geByClass = function(c){ var want = String(c).trim().split(/\s+/).filter(Boolean); return geWalk(this, function(e){ var cls = String(e.className || '').split(/\s+/); return want.every(function(w){ return cls.indexOf(w) >= 0; }); }); };
    var geByName = function(nm){ var want = String(nm); return geWalk(this, function(e){ return e.getAttribute && e.getAttribute('name') === want; }); };
    var addGetElems = function(proto){
      if (typeof proto.getElementsByTagName !== 'function') proto.getElementsByTagName = geByTag;
      if (typeof proto.getElementsByClassName !== 'function') proto.getElementsByClassName = geByClass;
      if (typeof proto.getElementsByName !== 'function') proto.getElementsByName = geByName;
    };
    addGetElems(baseProto);
    g.__addGetElems = addGetElems; // reused for document below
    // Traversal accessors the native binding lacks (it ships firstChild/nextSibling/children/
    // firstElementChild only). jQuery's clone-support probe reads clone.lastChild.checked; Sizzle
    // walks siblings. Derive from native childNodes/children/parentNode (all detached-safe).
    var idxIn = function(list, node){ for (var i=0;i<list.length;i++) if (list[i] === node) return i; return -1; };
    var defTrav = function(name, get){ if (baseProto && !Object.getOwnPropertyDescriptor(baseProto, name)) { try { Object.defineProperty(baseProto, name, { configurable: true, get: get }); } catch(e){} } };
    defTrav('lastChild', function(){ var c = this.childNodes || []; return c.length ? c[c.length-1] : null; });
    defTrav('lastElementChild', function(){ var c = this.children || []; return c.length ? c[c.length-1] : null; });
    defTrav('previousSibling', function(){ var p = this.parentNode; if (!p) return null; var c = p.childNodes || []; var i = idxIn(c, this); return i > 0 ? c[i-1] : null; });
    defTrav('previousElementSibling', function(){ var p = this.parentNode; if (!p) return null; var c = p.children || []; var i = idxIn(c, this); return i > 0 ? c[i-1] : null; });
    defTrav('nextElementSibling', function(){ var p = this.parentNode; if (!p) return null; var c = p.children || []; var i = idxIn(c, this); return (i >= 0 && i+1 < c.length) ? c[i+1] : null; });
    // classList on the shared element prototype (every element inherits it), backed by className.
    if (baseProto && !Object.getOwnPropertyDescriptor(baseProto, 'classList')) {
      try { Object.defineProperty(baseProto, 'classList', { configurable: true, get: function(){
        var el = this;
        var parse = function(){ return String(el.className || '').split(/\s+/).filter(Boolean); };
        var write = function(a){ el.className = a.join(' '); };
        var dt = {
          add: function(){ var a = parse(); for (var i=0;i<arguments.length;i++) if (a.indexOf(arguments[i])<0) a.push(arguments[i]); write(a); },
          remove: function(){ var a = parse(); for (var i=0;i<arguments.length;i++){ var x=a.indexOf(arguments[i]); if (x>=0) a.splice(x,1); } write(a); },
          toggle: function(c, force){ var a = parse(); var has = a.indexOf(c)>=0; var on = force === undefined ? !has : !!force; if (on && !has) a.push(c); else if (!on && has) a.splice(a.indexOf(c),1); write(a); return on; },
          contains: function(c){ return parse().indexOf(c)>=0; },
          replace: function(o,n){ var a = parse(); var x = a.indexOf(o); if (x<0) return false; a[x]=n; write(a); return true; },
          item: function(i){ return parse()[i] || null; },
          forEach: function(cb, thisArg){ parse().forEach(cb, thisArg); },
          toString: function(){ return String(el.className || ''); }
        };
        Object.defineProperty(dt, 'length', { get: function(){ return parse().length; } });
        Object.defineProperty(dt, 'value', { get: function(){ return String(el.className || ''); } });
        return dt;
      } }); } catch(e){}
    }
    var protoFor = {};
    var defType = { HTMLInputElement:'text', HTMLButtonElement:'submit' };
    ['HTMLInputElement','HTMLTextAreaElement','HTMLSelectElement','HTMLOptionElement','HTMLButtonElement','HTMLLabelElement','HTMLCanvasElement'].forEach(function(n){ if (typeof g[n] !== 'function') g[n] = function(){}; var p = Object.create(baseProto); try {
      Object.defineProperty(p, 'value', valDesc); Object.defineProperty(p, 'checked', checkedDesc); Object.defineProperty(p, 'form', formDesc);
      if (defType[n]) Object.defineProperty(p, 'type', mkTypeDesc(defType[n]));
      // Reflected IDL string attributes: a library setting `input.name = 'x'` (e.g. react-number-
      // format, MUI) must show up as the `name` content attribute (toHaveAttribute, [name] selectors).
      ['name','placeholder','accept','min','max','step','pattern','autocomplete'].forEach(function(a){ Object.defineProperty(p, a, { configurable: true, get: function(){ return this.getAttribute(a) || ''; }, set: function(v){ this.setAttribute(a, v == null ? '' : String(v)); } }); });
      // Reflected BOOLEAN IDL attributes (presence): el.disabled = true -> the `disabled` content
      // attribute; reading returns hasAttribute. Tests read el.disabled / [disabled] / multiple.
      [['disabled','disabled'],['multiple','multiple'],['required','required'],['readOnly','readonly'],['autoFocus','autofocus']].forEach(function(pair){ Object.defineProperty(p, pair[0], { configurable: true, get: function(){ return this.hasAttribute(pair[1]); }, set: function(v){ if (v) this.setAttribute(pair[1], ''); else this.removeAttribute(pair[1]); } }); });
      if (n === 'HTMLInputElement' || n === 'HTMLTextAreaElement') {
        Object.defineProperty(p, 'selectionStart', selStartDesc); Object.defineProperty(p, 'selectionEnd', selEndDesc); Object.defineProperty(p, 'selectionDirection', selDirDesc);
        p.setSelectionRange = function(s, e, dir){ this.__selStart = s; this.__selEnd = e; this.__selDir = dir || 'none'; };
        p.select = function(){ this.__selStart = 0; this.__selEnd = String(this.value||'').length; };
        p.setRangeText = function(){};
      }
      // click() on the interface prototype (input/textarea/button) so tests can spy on
      // HTMLInputElement.prototype.click (e.g. opening a file picker via inputRef.click()); the own
      // native click is dropped in createElement. NOT for select/option (their own click is
      // load-bearing for native dropdown behavior). Dispatches a bubbling click (checkbox/radio
      // activation + React onClick still run via el_dispatch_event).
      if (n === 'HTMLInputElement' || n === 'HTMLTextAreaElement' || n === 'HTMLButtonElement') {
        p.click = function(){ try { var C = g.MouseEvent || g.PointerEvent || g.Event; this.dispatchEvent(new C('click', { bubbles: true, cancelable: true })); } catch(e){} };
      }
    } catch(e){} g[n].prototype = p; protoFor[n] = p; });
    // Descendant <option>s. Prefer querySelectorAll (document order); fall back to a `children` walk
    // when it returns empty — that happens on a DETACHED subtree (qsa matches only connected nodes),
    // which is exactly when React-DOM's updateOptions reads select.options DURING commit (the subtree
    // is still detached then). Without the fallback a controlled <select> never marks its option.
    var optionsOf = function(sel){
      var a = []; var list = sel.querySelectorAll('option');
      for (var i=0;i<list.length;i++) a.push(list[i]);
      if (!a.length) (function walk(n){ var ch = n.children; for (var j=0;j<ch.length;j++){ var c = ch[j]; var tg = String(c.tagName).toUpperCase(); if (tg === 'OPTION') a.push(c); else if (tg === 'OPTGROUP') walk(c); } })(sel);
      a.item = function(i){ return this[i] || null; }; return a;
    };
    var ownerSelect = function(n){ var p = n.parentNode; while (p && p.nodeType === 1){ if (String(p.tagName).toUpperCase() === 'SELECT') return p; p = p.parentNode; } return null; };
    // <option>: `value` falls back to text content (HTML spec), `selected`/`defaultSelected` track
    // the live + attribute state. React-DOM's updateOptions reads option.value and writes
    // option.selected for every option of a controlled <select>.
    (function(){
      var op = protoFor.HTMLOptionElement;
      Object.defineProperty(op, 'value', { configurable: true, get: function(){ var v = this.getAttribute('value'); return v == null ? (this.textContent || '') : v; }, set: function(v){ this.setAttribute('value', v == null ? '' : String(v)); } });
      // Setting selected=true on a single (non-multiple) <select> deselects its siblings — the
      // browser enforces the single-selection invariant. Without this, an option marked during the
      // detached commit stays selected after userEvent.selectOptions marks another, and select.value
      // (first selected) returns the stale one.
      Object.defineProperty(op, 'selected', { configurable: true, get: function(){ return this.__selected === undefined ? this.hasAttribute('selected') : !!this.__selected; }, set: function(v){ v = !!v; this.__selected = v; if (v){ var s = ownerSelect(this); if (s && !s.multiple){ var os = optionsOf(s); for (var i=0;i<os.length;i++) if (os[i] !== this) os[i].__selected = false; } } } });
      Object.defineProperty(op, 'defaultSelected', { configurable: true, get: function(){ return this.hasAttribute('selected'); }, set: function(v){ if (v) this.setAttribute('selected',''); else this.removeAttribute('selected'); } });
      Object.defineProperty(op, 'disabled', { configurable: true, get: function(){ return this.hasAttribute('disabled'); }, set: function(v){ if (v) this.setAttribute('disabled',''); else this.removeAttribute('disabled'); } });
      Object.defineProperty(op, 'text', { configurable: true, get: function(){ return this.textContent || ''; }, set: function(v){ this.textContent = v; } });
    })();
    // <select>: `options` (HTMLOptionsCollection — the crash: React-DOM reads node.options.length),
    // plus `value`/`selectedIndex` derived from the options' selected state.
    (function(){
      var sp = protoFor.HTMLSelectElement;
      var opts = optionsOf;
      Object.defineProperty(sp, 'options', { configurable: true, get: function(){ return opts(this); } });
      Object.defineProperty(sp, 'selectedOptions', { configurable: true, get: function(){ return opts(this).filter(function(o){ return o.selected; }); } });
      Object.defineProperty(sp, 'selectedIndex', { configurable: true,
        get: function(){ var o = opts(this); for (var i=0;i<o.length;i++) if (o[i].selected) return i; return o.length ? -1 : -1; },
        set: function(idx){ var o = opts(this); for (var i=0;i<o.length;i++) o[i].selected = (i === idx); } });
      Object.defineProperty(sp, 'value', { configurable: true,
        get: function(){ var o = opts(this); for (var i=0;i<o.length;i++) if (o[i].selected) return o[i].value; return o.length ? o[0].value : ''; },
        set: function(v){ var o = opts(this); var s = String(v); var hit = false; for (var i=0;i<o.length;i++){ var m = (o[i].value === s); o[i].selected = m; if (m) hit = true; } if (!hit) for (var j=0;j<o.length;j++) o[j].selected = false; } });
    })();
    // <label>: htmlFor + control resolution. testing-library's getByLabelText filters
    // querySelectorAll('label') by `label.control === element`, so without `control` NO label ever
    // associates and every form field fails with "no form control was found associated to that label".
    (function(){
      var lp = protoFor.HTMLLabelElement;
      var labelable = function(el){ if (!el || el.nodeType !== 1) return false; var tg = String(el.tagName).toUpperCase(); if (tg === 'INPUT') return (el.getAttribute('type') || 'text').toLowerCase() !== 'hidden'; return tg === 'BUTTON' || tg === 'SELECT' || tg === 'TEXTAREA' || tg === 'METER' || tg === 'OUTPUT' || tg === 'PROGRESS'; };
      Object.defineProperty(lp, 'htmlFor', { configurable: true, get: function(){ return this.getAttribute('for') || ''; }, set: function(v){ this.setAttribute('for', v == null ? '' : String(v)); } });
      Object.defineProperty(lp, 'control', { configurable: true, get: function(){
        var f = this.getAttribute('for');
        if (f) { var el = this.ownerDocument.getElementById(f); return labelable(el) ? el : null; }
        var list = this.querySelectorAll('input, button, select, textarea, meter, output, progress');
        for (var i=0;i<list.length;i++) if (labelable(list[i])) return list[i];
        return null;
      } });
    })();
    var CTRL = { input:'HTMLInputElement', textarea:'HTMLTextAreaElement', select:'HTMLSelectElement', option:'HTMLOptionElement', button:'HTMLButtonElement', label:'HTMLLabelElement', canvas:'HTMLCanvasElement' };
    // <a> interface prototype with a `click` that dispatches (NOT an own per-instance method), so
    // download flows that spy on HTMLAnchorElement.prototype.click (createObjectURL + a.click) work —
    // an own native click would shadow the spy. Scoped to anchors only (the global variant regressed
    // select-like components, whose own native click is load-bearing).
    if (typeof g.HTMLAnchorElement !== 'function') g.HTMLAnchorElement = function(){};
    var anchorProto = Object.create(baseProto);
    anchorProto.click = function(){ try { var C = g.MouseEvent || g.PointerEvent || g.Event; this.dispatchEvent(new C('click', { bubbles: true, cancelable: true })); } catch(e){} };
    // HTMLHyperlinkElementUtils — origin/protocol/host/hostname/port/pathname/search/hash decomposed
    // from href (resolved against the document base). React/Next read anchorEl.origin during render;
    // undefined → "Cannot read properties of undefined (reading 'origin')". href already reflects.
    (function(){
      var anchorBase = function(){ return (g.location && g.location.href) || 'http://localhost/'; };
      var parse = function(el){ try { return new g.URL(el.href || '', anchorBase()); } catch(e){ return null; } };
      ['origin','protocol','host','hostname','port','pathname','search','hash','username','password'].forEach(function(p){
        Object.defineProperty(anchorProto, p, { configurable: true,
          get: function(){ var u = parse(this); if (!u) return p === 'pathname' ? '/' : ''; var v = u[p]; return v == null ? '' : v; },
          set: function(v){ var u = parse(this); if (!u) return; try { u[p] = v; this.href = u.href; } catch(e){} } });
      });
    })();
    g.HTMLAnchorElement.prototype = anchorProto;
    // Apply the control interface prototype + own value/checked to an element by its tag. Used by
    // createElement AND cloneNode (clones must keep type/value/checked/selected accessors — e.g.
    // userEvent's isValidDateOrTimeValue clones a date input, assigns, and checks the value stuck).
    // The DOM value-change tracker: browsers expose an internal "is the IDL value dirty since the
    // framework last saw it" signal that React/Preact/etc. read via the conventional `_valueTracker`
    // slot (a tracker is the standard contract, not React-private). We attach a spec-shaped one whose
    // `currentValue` advances only when value is set through the OWN accessor (programmatic / a
    // controlled-component restore) — NOT through the prototype accessor that user-event drives.
    // So user typing reads as "changed" (per-char onChange) while a controlled restore to the same
    // value reads as "unchanged" (no spurious trailing onChange — e.g. date inputs commit once).
    var attachValueTracker = function(el, field, protoDesc){
      var current = '' + (protoDesc.get ? protoDesc.get.call(el) : '');
      Object.defineProperty(el, field, { configurable: true, enumerable: false,
        get: function(){ return protoDesc.get.call(this); },
        set: function(v){ current = '' + v; protoDesc.set.call(this, v); } });
      // The tracker tracks the `value` field (text inputs). For checkbox/radio — which track `checked`
      // — el_set_attribute('type', …) strips this `value`+`_valueTracker` once the type is known, so
      // a framework falls back to its own checked path.
      el._valueTracker = { getValue: function(){ return current; }, setValue: function(v){ current = '' + v; }, stopTracking: function(){ el._valueTracker = null; try { Object.defineProperty(el, field, protoDesc); } catch(e){} } };
    };
    var applyControlProto = function(el){
      try {
        var t = String(el.tagName || '').toLowerCase();
        if (CTRL[t]) {
          Object.setPrototypeOf(el, protoFor[CTRL[t]]);
          // value uses a spec internal dirty-value slot (valDesc, distinct from the content attribute);
          // attach the DOM value-change tracker so framework change-detection works (per-char + dedup).
          if (t === 'input' || t === 'textarea') { attachValueTracker(el, 'value', valDesc); Object.defineProperty(el, 'checked', checkedDesc); }
          // drop the own native click on input/textarea/button so it resolves to the patchable proto
          // click (tests spy on HTMLInputElement.prototype.click for file pickers etc.).
          if ((t === 'input' || t === 'textarea' || t === 'button') && Object.prototype.hasOwnProperty.call(el, 'click')) delete el.click;
        } else if (t === 'a') {
          Object.setPrototypeOf(el, anchorProto);
          if (Object.prototype.hasOwnProperty.call(el, 'click')) delete el.click; // resolve to the patchable proto click
        }
      } catch(e){}
    };
    // <canvas>.getContext('2d') — a no-op 2D context stub (no rasterization). Covers components that
    // probe a context (signature pads, charts) without a real GPU/layout backend.
    var mkCanvasCtx = function(canvas){
      var noop = function(){};
      return {
        canvas: canvas,
        fillRect: noop, clearRect: noop, strokeRect: noop, beginPath: noop, closePath: noop,
        moveTo: noop, lineTo: noop, bezierCurveTo: noop, quadraticCurveTo: noop, arc: noop, arcTo: noop,
        rect: noop, ellipse: noop, fill: noop, stroke: noop, clip: noop, save: noop, restore: noop,
        scale: noop, rotate: noop, translate: noop, transform: noop, setTransform: noop, resetTransform: noop,
        drawImage: noop, putImageData: noop, setLineDash: noop, getLineDash: function(){ return []; },
        createLinearGradient: function(){ return { addColorStop: noop }; },
        createRadialGradient: function(){ return { addColorStop: noop }; },
        createPattern: function(){ return {}; },
        getImageData: function(x,y,w,h){ return { data: new Uint8ClampedArray(Math.max(0,(w||0)*(h||0)*4)), width: w||0, height: h||0 }; },
        createImageData: function(w,h){ return { data: new Uint8ClampedArray(Math.max(0,(w||0)*(h||0)*4)), width: w||0, height: h||0 }; },
        measureText: function(s){ return { width: (String(s||'').length)*6, actualBoundingBoxAscent: 8, actualBoundingBoxDescent: 2 }; },
        fillText: noop, strokeText: noop, isPointInPath: function(){ return false; },
        fillStyle: '#000', strokeStyle: '#000', lineWidth: 1, lineCap: 'butt', lineJoin: 'miter',
        font: '10px sans-serif', textAlign: 'start', textBaseline: 'alphabetic', globalAlpha: 1, globalCompositeOperation: 'source-over'
      };
    };
    // <canvas> methods live on HTMLCanvasElement.prototype (not own) so tests can mock
    // HTMLCanvasElement.prototype.getContext / getBoundingClientRect (signature pads) and the mock
    // isn't shadowed by an own method.
    (function(){
      var cp = protoFor.HTMLCanvasElement;
      cp.getContext = function(kind){ if (kind === '2d') { if (!this.__ctx2d) this.__ctx2d = mkCanvasCtx(this); return this.__ctx2d; } return null; };
      cp.toDataURL = function(){ return 'data:image/png;base64,'; };
      cp.toBlob = function(cb){ if (cb) cb(null); };
      cp.getBoundingClientRect = function(){ return { x:0, y:0, top:0, left:0, right:0, bottom:0, width:0, height:0, toJSON:function(){ return this; } }; };
    })();
    d.createElement = function(tag){
      var el = orig(tag); var t = String(tag).toLowerCase();
      try {
        if (t === 'style' && !el.sheet) { var s = mkSheet(el); Object.defineProperty(el, 'sheet', { configurable: true, get: function(){ return s; } }); sheets.push(s); }
      } catch(e){}
      applyControlProto(el);
      // canvas: drop the own native getBoundingClientRect so it resolves to the (mockable)
      // HTMLCanvasElement.prototype version; getContext/toDataURL/toBlob are on the proto too.
      if (t === 'canvas') { try { if (Object.prototype.hasOwnProperty.call(el, 'getBoundingClientRect')) delete el.getBoundingClientRect; } catch(e){} }
      return el;
    };
    // NOTE: cloneNode re-applies the control prototype natively (el_clone_node copies the source's
    // prototype to the clone), so a cloned <input>/<select> keeps its type/value/checked accessors
    // (userEvent's isValidDateOrTimeValue clones a date input, assigns, and checks the value stuck).
    void applyControlProto;
    if (!d.styleSheets) { try { Object.defineProperty(d, 'styleSheets', { configurable: true, get: function(){ return sheets; } }); } catch(e){} }

    // Alias the shim interface-constructor `.prototype`s onto the REAL element/document prototypes,
    // so a test patching HTMLElement.prototype.requestFullscreen / Document.prototype.exitFullscreen
    // (DataTableCore fullscreen) reaches our instances. Only affects methods that aren't own per
    // instance (own native methods still shadow). Default fullscreen stubs so components work
    // unmocked; document.fullscreenElement defaults null.
    try {
      if (g.HTMLElement) g.HTMLElement.prototype = baseProto;
      if (g.Element) g.Element.prototype = baseProto;
      if (g.Node) g.Node.prototype = baseProto;
      // Lazy `<style>.sheet` for EVERY <style> — server-rendered ones (from the HTML
      // parse / innerHTML) never pass through `createElement`, yet emotion /
      // styled-components hydrate by calling `styleEl.sheet.insertRule(...)` on them.
      // Without a `.sheet` that throws and aborts the chunk, blanking the styling.
      // `mkSheet`'s insertRule reflects into `el.textContent`, so the serialized HTML
      // then carries the injected CSS. Defined on the shared element prototype and
      // WITHOUT a descriptor guard (the NON_MASKING interceptor makes every lookup
      // report a descriptor, so a guard would wrongly skip). Caches per element.
      Object.defineProperty(baseProto, 'sheet', {
        configurable: true,
        get: function(){
          var tag = this.tagName;
          if (!tag || String(tag).toUpperCase() !== 'STYLE') return null;
          if (!this.__tcSheet) {
            try { this.__tcSheet = mkSheet(this); sheets.push(this.__tcSheet); }
            catch (e) { return mkSheet(this); }
          }
          return this.__tcSheet;
        }
      });
      var docProto = Object.getPrototypeOf(d) || baseProto;
      if (g.Document) g.Document.prototype = docProto;
      if (g.HTMLDocument) g.HTMLDocument.prototype = docProto;
      if (typeof baseProto.requestFullscreen !== 'function') baseProto.requestFullscreen = function(){ return Promise.resolve(); };
      if (typeof docProto.exitFullscreen !== 'function') docProto.exitFullscreen = function(){ return Promise.resolve(); };
      // NB: define these directly (NOT guarded by Object.getOwnPropertyDescriptor) — the all-Rust
      // DOM's NON_MASKING name interceptor intercepts every absent property (returns undefined), so
      // getOwnPropertyDescriptor synthesizes a truthy descriptor and a `if (!descriptor)` guard would
      // wrongly skip the define. A real own accessor still shadows the interceptor once defined.
      try { Object.defineProperty(d, 'fullscreenElement', { configurable: true, writable: true, value: null }); } catch(e){}
      // Document lifecycle/visibility — our document is always fully parsed and "foregrounded".
      // Components gate hydration / effects on these (readyState === 'complete', visibilityState).
      try { Object.defineProperty(d, 'readyState', { configurable: true, get: function(){ return 'complete'; } }); } catch(e){}
      try { Object.defineProperty(d, 'visibilityState', { configurable: true, get: function(){ return 'visible'; } }); } catch(e){}
      try { Object.defineProperty(d, 'hidden', { configurable: true, get: function(){ return false; } }); } catch(e){}
      // No hit-testing without layout → elementFromPoint(s) resolve to nothing.
      d.elementFromPoint = function(){ return null; };
      d.elementsFromPoint = function(){ return []; };
    } catch(e){}
  })();
  // ---- Range + Selection (real model over rtdom) ----------------------------------------------
  // The no-op stubs were enough for libraries that merely construct a Range, but contenteditable
  // editors (Lexical) and userEvent's contenteditable typing path REQUIRE a working selection:
  // userEvent.click sets the caret (createRange → setStart/End → selection.addRange), then on each
  // keypress userEvent reads selection.getRangeAt(0) to decide whether to emit `beforeinput`. With
  // a dead selection getInputRange() returns nothing and no input events fire, so onChange never
  // runs. This implements the slice of Range/Selection those paths exercise.
  function __zeroRect(){ return { x:0,y:0,top:0,left:0,right:0,bottom:0,width:0,height:0,toJSON:function(){return this;} }; }
  function __nodeIndex(n){ var p=n.parentNode; if(!p) return 0; var i=0,c=p.firstChild; while(c&&c!==n){ i++; c=c.nextSibling; } return i; }
  function __ancestry(n){ var a=[]; while(n){ a.push(n); n=n.parentNode; } return a; } // node … root
  // Compare boundary points (ca,oa) vs (cb,ob) in document order: -1 before, 0 equal, 1 after.
  function __cmp(ca,oa,cb,ob){
    if(ca===cb) return oa<ob?-1:oa>ob?1:0;
    var A=__ancestry(ca), B=__ancestry(cb);
    // walk B looking for a node whose ancestry includes it → common ancestor
    for(var i=0;i<A.length;i++){ var j=B.indexOf(A[i]); if(j>=0){
      var anc=A[i];
      // child of anc on A's side (or ca itself if A[i-1])
      var childA=i>0?A[i-1]:null, childB=j>0?B[j-1]:null;
      if(childA===null) return oa<=__nodeIndex(childB)?-1:1;     // ca IS the common ancestor
      if(childB===null) return __nodeIndex(childA)<ob?-1:1;       // cb IS the common ancestor
      var ia=__nodeIndex(childA), ib=__nodeIndex(childB);
      return ia<ib?-1:ia>ib?1:0;
    }}
    return 0; // disconnected
  }
  function __commonAncestor(a,b){ var A=__ancestry(a); var n=b; while(n){ if(A.indexOf(n)>=0) return n; n=n.parentNode; } return d; }
  function Range(){ this.startContainer=d; this.startOffset=0; this.endContainer=d; this.endOffset=0; }
  Object.defineProperties(Range.prototype, {
    collapsed:{ get:function(){ return this.startContainer===this.endContainer && this.startOffset===this.endOffset; } },
    commonAncestorContainer:{ get:function(){ return __commonAncestor(this.startContainer, this.endContainer); } },
  });
  Range.prototype.setStart=function(n,o){ this.startContainer=n; this.startOffset=o|0; if(__cmp(this.startContainer,this.startOffset,this.endContainer,this.endOffset)>0){ this.endContainer=n; this.endOffset=o|0; } };
  Range.prototype.setEnd=function(n,o){ this.endContainer=n; this.endOffset=o|0; if(__cmp(this.startContainer,this.startOffset,this.endContainer,this.endOffset)>0){ this.startContainer=n; this.startOffset=o|0; } };
  Range.prototype.setStartBefore=function(n){ this.setStart(n.parentNode, __nodeIndex(n)); };
  Range.prototype.setStartAfter=function(n){ this.setStart(n.parentNode, __nodeIndex(n)+1); };
  Range.prototype.setEndBefore=function(n){ this.setEnd(n.parentNode, __nodeIndex(n)); };
  Range.prototype.setEndAfter=function(n){ this.setEnd(n.parentNode, __nodeIndex(n)+1); };
  Range.prototype.collapse=function(toStart){ if(toStart){ this.endContainer=this.startContainer; this.endOffset=this.startOffset; } else { this.startContainer=this.endContainer; this.startOffset=this.endOffset; } };
  Range.prototype.selectNode=function(n){ this.setStartBefore(n); this.setEndAfter(n); };
  Range.prototype.selectNodeContents=function(n){ this.startContainer=n; this.startOffset=0; this.endContainer=n; this.endOffset=(n.nodeType===3?(n.data||'').length:(n.childNodes?n.childNodes.length:0)); };
  Range.prototype.cloneRange=function(){ var r=new Range(); r.startContainer=this.startContainer; r.startOffset=this.startOffset; r.endContainer=this.endContainer; r.endOffset=this.endOffset; return r; };
  Range.prototype.comparePoint=function(n,o){ if(__cmp(n,o,this.startContainer,this.startOffset)<0) return -1; if(__cmp(n,o,this.endContainer,this.endOffset)>0) return 1; return 0; };
  Range.prototype.isPointInRange=function(n,o){ return this.comparePoint(n,o)===0; };
  Range.prototype.compareBoundaryPoints=function(how,other){ return __cmp(this.startContainer,this.startOffset,other.startContainer,other.startOffset); };
  Range.prototype.insertNode=function(node){ var c=this.startContainer, o=this.startOffset; if(c.nodeType===3){ var txt=c.data||''; var after=c.splitText?c.splitText(o):null; c.parentNode.insertBefore(node, after||c.nextSibling); } else { var ref=c.childNodes?c.childNodes[o]:null; c.insertBefore(node, ref||null); } };
  Range.prototype.deleteContents=function(){ if(this.collapsed) return; var sc=this.startContainer; if(sc===this.endContainer && sc.nodeType===3){ var t=sc.data||''; if(sc.deleteData) sc.deleteData(this.startOffset, this.endOffset-this.startOffset); else sc.data=t.slice(0,this.startOffset)+t.slice(this.endOffset); this.endOffset=this.startOffset; this.endContainer=sc; return; } var ca=this.commonAncestorContainer; var kids=ca.childNodes?Array.prototype.slice.call(ca.childNodes):[]; for(var i=0;i<kids.length;i++){ var k=kids[i]; if(this.comparePoint(k,0)===0 && this.comparePoint(k, (k.nodeType===3?(k.data||'').length:(k.childNodes?k.childNodes.length:0)))===0 && k.parentNode){ k.parentNode.removeChild(k); } } this.collapse(true); };
  Range.prototype.cloneContents=function(){ var f=d.createDocumentFragment(); return f; };
  Range.prototype.extractContents=function(){ var f=this.cloneContents(); this.deleteContents(); return f; };
  Range.prototype.surroundContents=function(node){ this.insertNode(node); };
  Range.prototype.getClientRects=function(){ return []; };
  Range.prototype.getBoundingClientRect=function(){ return __zeroRect(); };
  Range.prototype.createContextualFragment=function(html){ var f=d.createDocumentFragment(); var t=d.createElement('div'); t.innerHTML=html; while(t.firstChild) f.appendChild(t.firstChild); return f; };
  Range.prototype.detach=function(){};
  Range.prototype.toString=function(){ var sc=this.startContainer; if(sc===this.endContainer && sc.nodeType===3){ return (sc.data||'').slice(this.startOffset, this.endOffset); } return ''; };
  d.createRange=function(){ var r=new Range(); r.startContainer=d.body||d; r.endContainer=d.body||d; return r; };
  if(g.Range===undefined) g.Range=Range;
  if (!d.getRootNode) d.getRootNode = function(){ return d; };
  // document.write / writeln: parse the written markup and append it to <body>. Real browsers splice
  // at the parser position; our scripts run post-parse, so appending is the faithful no-JS-engine
  // behavior (e.g. a script that writes one element per item).
  if (!d.write || !d.writeln) {
    var docWrite = function(){ var html = Array.prototype.join.call(arguments, ''); var tmp = d.createElement('div'); tmp.innerHTML = html; var target = d.body || d.documentElement; if (!target) return; var kids = Array.prototype.slice.call(tmp.childNodes); for (var i=0;i<kids.length;i++) target.appendChild(kids[i]); };
    if (!d.write) d.write = docWrite;
    if (!d.writeln) d.writeln = function(){ docWrite(Array.prototype.join.call(arguments, '') + '\n'); };
  }
  if (g.__addGetElems) { g.__addGetElems(d); try { delete g.__addGetElems; } catch(e){} }
  // ---- Selection (one live instance per document) ---------------------------------------------
  (function(){
    var ranges = [];
    var firing = false;
    function fireSelectionChange(){ if(firing) return; firing=true; try { var ev = { type:'selectionchange', bubbles:false, cancelable:false, target:d, currentTarget:null }; if (d.dispatchEvent) d.dispatchEvent(ev); } catch(e){} firing=false; }
    var sel = {
      get rangeCount(){ return ranges.length; },
      get anchorNode(){ return ranges.length?ranges[0].startContainer:null; },
      get anchorOffset(){ return ranges.length?ranges[0].startOffset:0; },
      get focusNode(){ return ranges.length?ranges[0].endContainer:null; },
      get focusOffset(){ return ranges.length?ranges[0].endOffset:0; },
      get isCollapsed(){ return ranges.length?ranges[0].collapsed:true; },
      get type(){ return ranges.length?(ranges[0].collapsed?'Caret':'Range'):'None'; },
      getRangeAt:function(i){ if(i>=ranges.length) throw new Error('IndexSizeError'); return ranges[i]; },
      addRange:function(r){ ranges=[r]; fireSelectionChange(); },
      removeAllRanges:function(){ if(ranges.length){ ranges=[]; fireSelectionChange(); } else ranges=[]; },
      empty:function(){ this.removeAllRanges(); },
      removeRange:function(r){ var i=ranges.indexOf(r); if(i>=0){ ranges.splice(i,1); fireSelectionChange(); } },
      collapse:function(node,offset){ if(!node){ this.removeAllRanges(); return; } var r=d.createRange(); r.setStart(node,offset|0); r.collapse(true); ranges=[r]; fireSelectionChange(); },
      collapseToStart:function(){ if(ranges.length){ ranges[0].collapse(true); fireSelectionChange(); } },
      collapseToEnd:function(){ if(ranges.length){ ranges[0].collapse(false); fireSelectionChange(); } },
      extend:function(node,offset){ if(!ranges.length){ this.collapse(node,offset); return; } ranges[0].setEnd(node,offset|0); fireSelectionChange(); },
      setBaseAndExtent:function(an,ao,fn,fo){ var r=d.createRange(); if(__cmp(an,ao|0,fn,fo|0)<=0){ r.setStart(an,ao|0); r.setEnd(fn,fo|0); } else { r.setStart(fn,fo|0); r.setEnd(an,ao|0); } ranges=[r]; fireSelectionChange(); },
      selectAllChildren:function(node){ var r=d.createRange(); r.selectNodeContents(node); ranges=[r]; fireSelectionChange(); },
      containsNode:function(node, partial){ if(!ranges.length) return false; var r=ranges[0]; var a=r.comparePoint(node,0), b=r.comparePoint(node,(node.nodeType===3?(node.data||'').length:(node.childNodes?node.childNodes.length:0))); return partial ? (a<=0&&b>=0) : (a===0&&b===0); },
      deleteFromDocument:function(){ if(ranges.length) ranges[0].deleteContents(); },
      modify:function(){},
      toString:function(){ return ranges.length?ranges[0].toString():''; },
    };
    d.getSelection = function(){ return sel; };
    if (!g.getSelection) g.getSelection = function(){ return sel; };
  })();
})();
