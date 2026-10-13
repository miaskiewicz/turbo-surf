// Nullable/enumerated HTML reflection and genuine ToggleEvent construction.
// Top-layer rendering and opening/closing policy require separate host support.
(function () {
  'use strict';
  const g = globalThis, ns = g[Symbol.for('ts')], make = ns.makeNativeFunction, StringValue = String, ObjectValue = Object;
  const check = Object.getOwnPropertyDescriptor(g.Node.prototype, 'nodeType').get;
  const namespace = Object.getOwnPropertyDescriptor(g.Element.prototype, 'namespaceURI').get;
  const get = g.Element.prototype.getAttribute, set = g.Element.prototype.setAttribute, remove = g.Element.prototype.removeAttribute;
  const string = value => { if (typeof value === 'symbol') throw new TypeError('Cannot convert a Symbol to a string'); return StringValue(value); };
  const html = receiver => { if (check.call(receiver) !== 1 || namespace.call(receiver) !== 'http://www.w3.org/1999/xhtml') throw new TypeError('Illegal invocation'); };
  Object.defineProperty(g.HTMLElement.prototype, 'popover', {
    enumerable: true, configurable: true,
    get: make(function () {
      html(this); const value = get.call(this, 'popover');
      if (value === null) return null;
      const folded = value.replace(/[A-Z]/g, character => character.toLowerCase());
      return folded === '' ? 'auto' : ['auto', 'manual', 'hint'].includes(folded) ? folded : 'manual';
    }, 'get popover', 0),
    set: make(function (value) { html(this); if (value === null) remove.call(this, 'popover'); else set.call(this, 'popover', string(value)); }, 'set popover', 1),
  });
  if (typeof g.ToggleEvent === 'undefined') {
    const Event = g.Event, construct = Reflect.construct;
    const records = new WeakMap(), read = records.get.bind(records), store = records.set.bind(records);
    const Delegate = function (type, init) {
      if (!new.target) throw new TypeError('ToggleEvent requires new');
      if (!arguments.length) throw new TypeError('ToggleEvent requires 1 argument');
      init = init == null ? {} : ObjectValue(init);
      const source = init.source === undefined || init.source === null ? null : init.source;
      if (source !== null && check.call(source) !== 1) throw new TypeError('ToggleEvent source must be an Element');
      const oldState = init.oldState === undefined ? '' : string(init.oldState), newState = init.newState === undefined ? '' : string(init.newState);
      const event = construct(Event, [string(type), init], new.target);
      store(event, { oldState, newState, source });
      return event;
    };
    Object.setPrototypeOf(Delegate.prototype, Event.prototype);
    Object.defineProperty(Delegate.prototype, Symbol.toStringTag, { value: 'ToggleEvent', configurable: true });
    for (const name of ['oldState', 'newState', 'source']) Object.defineProperty(Delegate.prototype, name, {
      enumerable: true, configurable: true,
      get: make(function () { const record = read(this); if (!record) throw new TypeError('Illegal invocation'); return record[name]; }, 'get ' + name, 0),
    });
    const constructor = make(Delegate, 'ToggleEvent', 1, true, true);
    constructor.prototype = Delegate.prototype;
    Object.defineProperty(Delegate.prototype, 'constructor', { value: constructor, writable: true, configurable: true });
    Object.defineProperty(constructor, 'prototype', { writable: false });
    Object.setPrototypeOf(constructor, Event);
    Object.defineProperty(g, 'ToggleEvent', { value: constructor, writable: true, configurable: true });
  }
})();
