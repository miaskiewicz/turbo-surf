(() => {
  const rows = {};
  const attempt = (callback) => {
    try {
      return callback();
    } catch (error) {
      return { error: error.name };
    }
  };
  const node = document.createElementNS("urn:probe:element", "p:Mixed");
  const other = document.createElement("div");
  node.setAttributeNS("urn:probe:first", "one:value", "first");
  node.setAttributeNS("urn:probe:second", "two:value", "second");
  node.setAttribute("value", "plain");
  const attribute = node.getAttributeNodeNS("urn:probe:first", "value");
  const map = node.attributes;
  rows.element = [
    node.namespaceURI,
    node.prefix,
    node.localName,
    node.tagName,
    node instanceof Element,
    node instanceof HTMLElement,
  ];
  rows.attribute = [
    attribute.namespaceURI,
    attribute.prefix,
    attribute.localName,
    attribute.name,
    attribute.nodeName,
    attribute.nodeType,
    attribute instanceof Attr,
    attribute instanceof Node,
    attribute.ownerElement === node,
    attribute.ownerDocument === document,
    attribute === node.getAttributeNode("one:value"),
    attribute === map[0],
  ];
  attribute.value = "live";
  rows.values = [
    node.getAttributeNS("urn:probe:first", "value"),
    node.getAttributeNS("urn:probe:second", "value"),
    node.getAttributeNS(null, "value"),
    node.getAttributeNS("urn:absent", "value"),
    attribute.nodeValue,
    attribute.textContent,
  ];
  rows.map = [
    map === node.attributes,
    Array.isArray(map),
    Object.prototype.toString.call(map),
    map instanceof NamedNodeMap,
    map.item(0) === attribute,
    map.getNamedItemNS("urn:probe:first", "value") === attribute,
    Array.from(map, (entry) => entry.name),
  ];
  rows.errors = [
    attempt(() => node.setAttributeNS(null, "p:value", "bad")),
    attempt(() => node.setAttributeNS("urn:wrong", "xml:lang", "bad")),
    attempt(() => document.createElementNS("urn:wrong", "xmlns:item")),
    attempt(() => other.setAttributeNode(attribute)),
    attempt(() => other.setAttributeNode({ name: "fake", value: "bad" })),
    attempt(() => node.appendChild(attribute)),
    attempt(() => new Attr()),
    attempt(() => new NamedNodeMap()),
  ];
  const replacement = document.createAttributeNS("urn:probe:first", "new:value");
  replacement.value = "replaced";
  rows.replacement = [
    map.setNamedItemNS(replacement) === attribute,
    attribute.ownerElement === null,
    attribute.value,
    map.getNamedItemNS("urn:probe:first", "value") === replacement,
  ];
  rows.removal = [
    map.removeNamedItemNS("urn:probe:first", "value") === replacement,
    replacement.ownerElement === null,
    replacement.value,
    !node.hasAttributeNS("urn:probe:first", "value"),
    map.length,
  ];
  const child = document.createElementNS(null, "child");
  node.appendChild(child);
  node.setAttributeNS("http://www.w3.org/2000/xmlns/", "xmlns:q", "urn:probe:lookup");
  rows.lookup = [
    child.lookupNamespaceURI("q"),
    child.lookupPrefix("urn:probe:lookup"),
    child.lookupNamespaceURI("xml"),
    node.lookupNamespaceURI("p"),
    node.getAttributeNodeNS("http://www.w3.org/2000/xmlns/", "q").lookupNamespaceURI("q"),
  ];
  const copy = node.cloneNode(true);
  rows.clone = [
    copy.namespaceURI,
    copy.prefix,
    copy.localName,
    copy.getAttributeNS("urn:probe:second", "value"),
    copy.firstChild.namespaceURI,
  ];
  rows.brands = [
    attempt(() => Object.getOwnPropertyDescriptor(Attr.prototype, "value").get.call({})),
    attempt(() => Object.getOwnPropertyDescriptor(NamedNodeMap.prototype, "length").get.call({})),
  ];
  return JSON.stringify(rows);
})();
