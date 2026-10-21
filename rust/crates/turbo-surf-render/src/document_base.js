// Base URLs freeze when the relevant native tree mutation is processed. Later
// policies cannot retroactively change an accepted base, but later href writes can.
const __baseApply = Reflect.apply;
const __baseQuery = Document.prototype.querySelectorAll;
const __baseSubtreeQuery = Element.prototype.querySelectorAll;
const __baseAttribute = Element.prototype.getAttribute;
const __baseOwner = __G.domAccessor("ownerDocument", false, "Node");
const __baseParent = __G.domAccessor("parentNode", false, "Node");
const __baseLocal = __G.domAccessor("localName", false, "Element");
const __baseNamespace = __G.domAccessor("namespaceURI", false, "Element");
const __baseRows = __interfaceMap("Document.frozen-base");
const __baseLocation = globalThis.location;
const __baseLocationHref = ops.op_native_location_installed()
  ? () => ops.op_document_location_url(__document)
  : Object.getOwnPropertyDescriptor(__baseLocation, "href").get;
__documentRuntimeState.baseFallback = () =>
  __documentRuntimeState.frame
    ? __documentRuntimeState.frame.url
    : __baseApply(__baseLocationHref, __baseLocation, []);
function __baseFallback(doc) {
  const state = __documentState(doc);
  return state.inactive
    ? state.url
    : state.baseFallback
      ? state.baseFallback()
      : state.frame
        ? state.frame.url
        : state.url;
}
function __htmlPolicyElement(node, kind) {
  return (
    __baseApply(__documentType, node, []) === 1 &&
    __baseApply(__baseLocal, node, []) === kind &&
    __baseApply(__baseNamespace, node, []) === "http://www.w3.org/1999/xhtml"
  );
}
function __firstDocumentBase(doc) {
  const rows = __baseApply(__baseQuery, doc, ["base[href]"]);
  for (let index = 0; index < rows.length; index++)
    if (__htmlPolicyElement(rows[index], "base")) return rows[index];
  return null;
}
function __baseInHead(node) {
  for (
    let parent = __baseApply(__baseParent, node, []);
    parent;
    parent = __baseApply(__baseParent, parent, [])
  )
    if (__htmlPolicyElement(parent, "head")) return true;
  return false;
}
function __setFrozenBase(doc, element, initial) {
  const fallback = __baseFallback(doc);
  const href = element ? __baseApply(__baseAttribute, element, ["href"]) : null;
  const value =
    href === null
      ? fallback
      : ops.op_csp_base_url(doc, href, fallback, initial, !__documentState(doc).inactive);
  __baseRows.set(doc, { __proto__: null, element, value });
}
function __initializeDocumentBase() {
  __setFrozenBase(__document, __firstDocumentBase(__document), true);
}
function __documentBaseMutation(event, script) {
  const doc = __document,
    current = __firstDocumentBase(doc),
    previous = __baseRows.get(doc),
    nodes = { __proto__: null, length: 0 };
  const candidates = event.scriptCandidates,
    handled = { __proto__: null };
  let connected = false;
  for (let index = 0; index < event.ancestors.length; index++)
    if (event.ancestors[index] === doc) connected = true;
  // Parser/compound algorithms flush construction records after insertion. Their
  // original disconnected attribute writes did not process connected metadata.
  if (!connected) {
    for (let index = 0; index < candidates.length; index++) script(candidates[index]);
    return;
  }

  if (event.type === "childList") {
    const added = event.added;
    for (let index = 0; index < added.length; index++) {
      const node = added[index];
      nodes[nodes.length++] = node;
      if (__baseApply(__documentType, node, []) !== 1) continue;
      const children = __baseApply(__baseSubtreeQuery, node, ["meta,base,script"]);
      for (let child = 0; child < children.length; child++) nodes[nodes.length++] = children[child];
    }
  } else nodes[nodes.length++] = event.target;
  let inserted = false;
  for (let index = 0; index < nodes.length; index++) if (nodes[index] === current) inserted = true;
  // Base insertion steps precede postconnection meta/script processing for a
  // compound fragment, even when the fragment's meta occurs before its base.
  if (
    !previous ||
    previous.element !== current ||
    (event.type === "childList" && inserted) ||
    (event.type === "attributes" && event.target === current && event.attributeName === "href")
  )
    __setFrozenBase(doc, current, false);
  for (let index = 0; index < nodes.length; index++) {
    const node = nodes[index];
    if (
      __htmlPolicyElement(node, "meta") &&
      (event.type === "childList" ||
        event.attributeName === "name" ||
        event.attributeName === "content")
    )
      ops.op_referrer_meta(doc, node);
    if (
      __htmlPolicyElement(node, "meta") &&
      __baseInHead(node) &&
      (event.type === "childList" ||
        event.attributeName === "content" ||
        event.attributeName === "http-equiv")
    )
      ops.op_csp_meta(doc, node);
    for (let candidate = 0; candidate < candidates.length; candidate++)
      if (candidates[candidate] === node) {
        handled[candidate] = true;
        script(node);
      }
  }
  for (let candidate = 0; candidate < candidates.length; candidate++)
    if (!handled[candidate]) script(candidates[candidate]);
}
const __baseURI = Object.getOwnPropertyDescriptor(
  {
    get baseURI() {
      const kind = __baseApply(__documentType, this, []);
      const doc = kind === 9 ? this : __baseApply(__baseOwner, this, []);
      if (!doc) return "";
      const state = __documentState(doc),
        row = __baseRows.get(doc);
      if (!state.inactive && row) return row.element ? row.value : __baseFallback(doc);
      const first = __firstDocumentBase(doc),
        fallback = __baseFallback(doc);
      if (!first) return fallback;
      return ops.op_csp_base_url(
        doc,
        __baseApply(__baseAttribute, first, ["href"]),
        fallback,
        false,
        !state.inactive,
      );
    },
  },
  "baseURI",
).get;
Object.defineProperty(Node.prototype, "baseURI", {
  get: __documentFunction(__baseURI, "get baseURI", 0),
  configurable: true,
  enumerable: true,
});
if (Object.hasOwn(__document, "baseURI")) delete __document.baseURI;
