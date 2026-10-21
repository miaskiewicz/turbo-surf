(() => {
  const doc = new DOMParser().parseFromString(
    '<!doctype html><html><head><title> A  B </title></head><body><form id="first" name="named"></form><img><a href="/"></a><a name="anchor"></a><embed></body></html>',
    "text/html",
  );
  const list = doc.forms;
  const result = {};
  const describe = (value) =>
    value && {
      writable: value.writable,
      enumerable: value.enumerable,
      configurable: value.configurable,
    };
  result.collection = {
    tag: Object.prototype.toString.call(list),
    array: Array.isArray(list),
    same: list === doc.forms,
    names: [
      list.first === list[0],
      list.named === list[0],
      list.namedItem("named") === list[0],
      list.namedItem(""),
    ],
    keys: Object.keys(list),
    ownKeys: Reflect.ownKeys(list),
    index: describe(Object.getOwnPropertyDescriptor(list, "0")),
    name: describe(Object.getOwnPropertyDescriptor(list, "first")),
  };
  const first = list[0];
  result.collection.mutation = [
    Reflect.set(list, "0", null),
    list[0] === first,
    Reflect.defineProperty(list, "0", { value: null }),
    list[0] === first,
    Reflect.deleteProperty(list, "0"),
  ];
  const childNodes = doc.body.childNodes,
    children = doc.body.children,
    staticNodes = doc.querySelectorAll("form"),
    namedNodes = doc.getElementsByName("named"),
    byTag = doc.getElementsByTagName("form");
  const extra = doc.createElement("form");
  doc.body.appendChild(extra);
  first.setAttribute("name", "changed");
  result.live = [
    list.length,
    children.length,
    childNodes.length,
    staticNodes.length,
    namedNodes.length,
    byTag.length,
    childNodes === doc.body.childNodes,
    children === doc.body.children,
    childNodes instanceof NodeList,
    staticNodes instanceof NodeList,
    byTag instanceof HTMLCollection,
  ];
  result.document = [
    doc.compatMode,
    doc.doctype.name,
    doc.doctype.publicId,
    doc.doctype.systemId,
    doc.doctype.nodeName,
    Object.prototype.toString.call(doc.doctype),
    doc.images.length,
    doc.links.length,
    doc.anchors.length,
    doc.embeds.length,
    doc.applets.length,
    doc.embeds === doc.plugins,
    doc.title,
  ];
  doc.doctype.remove();
  doc.designMode = "ON";
  doc.designMode = "invalid";
  doc.dir = "RTL";
  doc.bgColor = "red";
  doc.fgColor = "green";
  result.reflected = [
    doc.doctype,
    doc.compatMode,
    doc.designMode,
    doc.dir,
    doc.documentElement.getAttribute("dir"),
    doc.bgColor,
    doc.body.getAttribute("bgcolor"),
    doc.fgColor,
  ];
  const target = new DOMParser().parseFromString("<!doctype html><body></body>", "text/html");
  const element = doc.createElement("div"),
    input = doc.createElement("input");
  input.value = "dirty";
  element.append(input);
  doc.body.append(element);
  let count = 0;
  input.addEventListener("change", () => count++);
  const originalChild = element.firstChild,
    adopted = target.adoptNode(element);
  target.body.append(adopted);
  input.dispatchEvent(new Event("change"));
  result.adoption = [
    adopted === element,
    input === originalChild,
    element.ownerDocument === target,
    input.ownerDocument === target,
    input.value,
    count,
    doc.querySelector("input") === null,
    target.querySelector("input") === input,
  ];
  const nonce = doc.createElement("script");
  result.nonce = [];
  nonce.setAttribute("nonce", "attribute");
  result.nonce.push([nonce.nonce, nonce.getAttribute("nonce")]);
  nonce.nonce = "private";
  result.nonce.push([nonce.nonce, nonce.getAttribute("nonce"), nonce.cloneNode().nonce]);
  nonce.setAttribute("nonce", "updated");
  result.nonce.push([nonce.nonce, nonce.getAttribute("nonce")]);
  nonce.removeAttribute("nonce");
  result.nonce.push([nonce.nonce, nonce.getAttribute("nonce")]);
  const order = [];
  doc.onabort = () => order.push("first");
  doc.addEventListener("abort", () => order.push("listener"));
  doc.onabort = () => order.push("replacement");
  doc.dispatchEvent(new Event("abort"));
  result.events = order;
  result.namespaces = [
    doc.getElementsByTagNameNS("http://www.w3.org/1999/xhtml", "form").length,
    doc.getElementsByTagNameNS("*", "*").length,
    doc.getElementsByTagNameNS(null, "*").length,
  ];
  return JSON.stringify(result);
})();
