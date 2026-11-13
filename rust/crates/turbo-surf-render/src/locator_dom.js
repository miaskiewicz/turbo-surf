// Automation alone pierces open roots. Capture native accessors before page code
// can replace them; cached wrappers are private and tied to the native revision.
const __locatorApply = Reflect.apply;
const __locatorType = Object.getOwnPropertyDescriptor(Node.prototype, "nodeType").get;
const __locatorParent = Object.getOwnPropertyDescriptor(Node.prototype, "parentElement").get;
const __locatorRoot = Node.prototype.getRootNode;
const __locatorShadow = Object.getOwnPropertyDescriptor(Element.prototype, "shadowRoot").get;
const __locatorHost = Object.getOwnPropertyDescriptor(ShadowRoot.prototype, "host").get;
const __locatorPrevious = Object.getOwnPropertyDescriptor(
  Element.prototype,
  "previousElementSibling",
).get;
const __locatorMatches = Element.prototype.matches;
const __locatorDocumentId = Document.prototype.getElementById;
const __locatorFragmentId = DocumentFragment.prototype.getElementById;
const __locatorControl = Object.getOwnPropertyDescriptor(HTMLLabelElement.prototype, "control").get;
const __locatorLength = Object.getOwnPropertyDescriptor(NodeList.prototype, "length").get;
const __locatorItem = NodeList.prototype.item;
const __locatorPush = Function.prototype.call.bind(Array.prototype.push);
const __locatorPop = Function.prototype.call.bind(Array.prototype.pop);
const __locatorSlice = Function.prototype.call.bind(Array.prototype.slice);
const __locatorFilter = Function.prototype.call.bind(Array.prototype.filter);
const __locatorSet = Set;
const __locatorAdd = Function.prototype.call.bind(Set.prototype.add);
const __locatorHas = Function.prototype.call.bind(Set.prototype.has);
const __locatorQueryMethods = {
  __proto__: null,
  1: Element.prototype.querySelectorAll,
  9: Document.prototype.querySelectorAll,
  11: DocumentFragment.prototype.querySelectorAll,
};
function __locatorNativeQuery(root, selector) {
  const type = __locatorApply(__locatorType, root, []);
  const list = __locatorApply(__locatorQueryMethods[type], root, [selector]);
  const result = __list();
  const length = __locatorApply(__locatorLength, list, []);
  for (let index = 0; index < length; index++)
    __locatorPush(result, __locatorApply(__locatorItem, list, [index]));
  return result;
}
let __locatorRevision;
let __locatorIndex = __list();
let __locatorHasShadow = false;
function __locatorShadowOf(element, pending) {
  const shadow = __locatorApply(__locatorShadow, element, []);
  if (shadow) __locatorPush(pending, shadow);
  return !!shadow;
}
function __locatorRefresh() {
  const revision = ops.op_locator_revision(__document);
  if (revision !== null && revision === __locatorRevision) return;
  __locatorIndex = __list();
  __locatorHasShadow = false;
  const pending = __list();
  __locatorPush(pending, __document);
  while (pending.length) {
    const elements = __locatorNativeQuery(__locatorPop(pending), "*");
    for (let index = 0; index < elements.length; index++)
      __locatorPush(__locatorIndex, elements[index]);
    // A stack visits earlier hosts first when roots are pushed in reverse.
    for (let index = elements.length - 1; index >= 0; index--)
      if (__locatorShadowOf(elements[index], pending)) __locatorHasShadow = true;
  }
  __locatorRevision = revision;
}
__G.tcLocatorHasShadow = () => {
  if (!ops.op_locator_has_shadow(__document)) return false;
  __locatorRefresh();
  return __locatorHasShadow;
};
__G.tcLocatorAll = () => {
  __locatorRefresh();
  return __locatorSlice(__locatorIndex);
};
__G.tcLocatorParent = (element) => {
  const parent = __locatorApply(__locatorParent, element, []);
  if (parent) return parent;
  const root = __locatorApply(__locatorRoot, element, []);
  return __locatorApply(__locatorType, root, []) === 11
    ? __locatorApply(__locatorHost, root, [])
    : null;
};
__G.tcLocatorContains = (root, element) => {
  for (let current = element; current; current = __G.tcLocatorParent(current))
    if (current === root) return true;
  return root === __document && __resourceNativeOwner(element) === __document;
};
__G.tcLocatorQuery = (root, selector) => {
  const selected = new __locatorSet();
  const pending = __list();
  __locatorPush(pending, root);
  while (pending.length) {
    const current = __locatorPop(pending);
    const matches = __locatorNativeQuery(current, selector);
    for (let index = 0; index < matches.length; index++) __locatorAdd(selected, matches[index]);
    if (__locatorApply(__locatorType, current, []) === 1) __locatorShadowOf(current, pending);
    const children = __locatorNativeQuery(current, "*");
    for (let index = 0; index < children.length; index++)
      __locatorShadowOf(children[index], pending);
  }
  __locatorRefresh();
  return __locatorFilter(__locatorIndex, (element) => __locatorHas(selected, element));
};
__G.tcLocatorDom = {
  all: (root) => {
    __locatorRefresh();
    return __locatorFilter(
      __locatorIndex,
      (element) => element !== root && __G.tcLocatorContains(root, element),
    );
  },
  contains: __G.tcLocatorContains,
  validate: (root, selector) => {
    const type = __locatorApply(__locatorType, root, []);
    __locatorApply(__locatorQueryMethods[type], root, [selector]);
  },
  parent: __G.tcLocatorParent,
  previous: (element) => __locatorApply(__locatorPrevious, element, []),
  matches: (element, selector) => __locatorApply(__locatorMatches, element, [selector]),
};

__G.tcLocatorById = (element, id) => {
  const root = __locatorApply(__locatorRoot, element, []);
  const method =
    __locatorApply(__locatorType, root, []) === 9 ? __locatorDocumentId : __locatorFragmentId;
  return __locatorApply(method, root, [id]);
};
__G.tcLocatorLabelControl = (label) => __locatorApply(__locatorControl, label, []);
__G.tcLocatorLightQuery = (element, selector) =>
  __locatorSlice(__locatorNativeQuery(__locatorApply(__locatorRoot, element, []), selector));

// Metadata keeps each actual tree scope intact. Imports use an inert document
// with no custom-element registry, and copy only light children. Reading a
// locator must neither upgrade application elements nor flatten nested roots.
const __locatorImplementation = __document.implementation;
const __locatorCreateDocument = DOMImplementation.prototype.createHTMLDocument;
const __locatorCreateElement = Document.prototype.createElement;
const __locatorImport = Document.prototype.importNode;
const __locatorAppend = Node.prototype.appendChild;
const __locatorChildNodes = Object.getOwnPropertyDescriptor(Node.prototype, "childNodes").get;
const __locatorDocumentElement = Object.getOwnPropertyDescriptor(
  Document.prototype,
  "documentElement",
).get;
const __locatorSetAttribute = Element.prototype.setAttribute;
const __locatorOuter = Object.getOwnPropertyDescriptor(Element.prototype, "outerHTML").get;
const __locatorMap = Map;
const __locatorMapGet = Function.prototype.call.bind(Map.prototype.get);
const __locatorMapSet = Function.prototype.call.bind(Map.prototype.set);
const __locatorUnshift = Function.prototype.call.bind(Array.prototype.unshift);
let __locatorMetadataDocument;
function __locatorMetadataOwner() {
  if (!__locatorMetadataDocument)
    __locatorMetadataDocument = __locatorApply(__locatorCreateDocument, __locatorImplementation, [
      "",
    ]);
  return __locatorMetadataDocument;
}
function __locatorChildren(node) {
  const children = __locatorApply(__locatorChildNodes, node, []);
  const length = __locatorApply(__locatorLength, children, []);
  const result = __list();
  for (let index = 0; index < length; index++)
    __locatorPush(result, __locatorApply(__locatorItem, children, [index]));
  return result;
}
function __locatorInertCopy(source) {
  const document = __locatorMetadataOwner();
  const copy = __locatorApply(__locatorImport, document, [source, false]);
  const pending = __list();
  __locatorPush(pending, { __proto__: null, source, copy });
  while (pending.length) {
    const current = __locatorPop(pending);
    const children = __locatorChildren(current.source);
    for (let index = 0; index < children.length; index++) {
      const child = __locatorApply(__locatorImport, document, [children[index], false]);
      __locatorApply(__locatorAppend, current.copy, [child]);
      __locatorPush(pending, { __proto__: null, source: children[index], copy: child });
    }
  }
  return copy;
}
function __locatorMetadataRoot(root) {
  const document = __locatorApply(__locatorType, root, []) === 9;
  let copy;
  if (document) {
    copy = __locatorInertCopy(__locatorApply(__locatorDocumentElement, root, []));
  } else {
    copy = __locatorApply(__locatorCreateElement, __locatorMetadataOwner(), ["div"]);
    const children = __locatorChildren(root);
    for (let index = 0; index < children.length; index++)
      __locatorApply(__locatorAppend, copy, [__locatorInertCopy(children[index])]);
  }
  const source = __locatorNativeQuery(root, "*");
  const copies = __locatorNativeQuery(copy, "*");
  if (document) __locatorUnshift(copies, copy);
  const nodes = new __locatorMap();
  for (let index = 0; index < source.length; index++)
    __locatorMapSet(nodes, source[index], copies[index]);
  return { __proto__: null, copy, nodes, index: 0 };
}
__G.tcLocatorSnapshots = (elements, marker) => {
  const roots = new __locatorMap();
  const groups = __list();
  const membership = __list();
  for (let index = 0; index < elements.length; index++) {
    const root = __locatorApply(__locatorRoot, elements[index], []);
    let group = __locatorMapGet(roots, root);
    if (!group) {
      group = __locatorMetadataRoot(root);
      group.index = groups.length;
      __locatorPush(groups, group);
      __locatorMapSet(roots, root, group);
    }
    const copy = __locatorMapGet(group.nodes, elements[index]);
    if (!copy) throw new Error("Native locator metadata source changed");
    __locatorApply(__locatorSetAttribute, copy, [marker, __String(index)]);
    __locatorPush(membership, group.index);
  }
  const html = __list();
  for (let index = 0; index < groups.length; index++)
    __locatorPush(html, __locatorApply(__locatorOuter, groups[index].copy, []));
  return {
    __proto__: null,
    html: __locatorSlice(html),
    membership: __locatorSlice(membership),
    baseURL: __resourceNativeBase(__document),
  };
};
