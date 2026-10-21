(() => {
  const result = {};
  const error = (action) => {
    try {
      action();
      return null;
    } catch (e) {
      return [e.name, e.code, e instanceof DOMException];
    }
  };
  const doc = new DOMParser().parseFromString("<!doctype html><body>", "text/html");
  result.metadata = ["xmlEncoding", "xmlStandalone", "xmlVersion", "rootElement", "compatMode"].map(
    (key) => [key, doc[key]],
  );
  result.cdata = error(() => doc.createCDATASection("text"));
  result.invalid = ["bad target", "1target", ":target", "xml", "é", "a:b", "a\u0301", "\ud800"].map(
    (target) => [
      target === "\ud800" ? "lone-surrogate" : target,
      error(() => doc.createProcessingInstruction(target, "data")),
    ],
  );
  result.terminator = error(() => doc.createProcessingInstruction("test", "?>"));
  result.receiver = error(() =>
    Document.prototype.createProcessingInstruction.call({}, "test", "data"),
  );
  const pi = doc.createProcessingInstruction("test", "a😀b");
  result.initial = [
    pi.nodeType,
    pi.nodeName,
    pi.target,
    pi.data,
    pi.nodeValue,
    pi.textContent,
    pi.length,
    pi instanceof ProcessingInstruction,
    pi instanceof CharacterData,
    Object.prototype.toString.call(pi),
  ];
  pi.insertData(3, "!");
  result.edit = [pi.data, pi.substringData(1, 2), error(() => pi.deleteData(99, 1))];
  doc.body.appendChild(pi);
  const clone = pi.cloneNode();
  result.clone = [clone !== pi, clone.target, clone.data, clone.ownerDocument === doc];
  result.parent = [pi.parentNode === doc.body, doc.body.textContent, doc.body.innerHTML];
  const other = new DOMParser().parseFromString("<body>", "text/html");
  const imported = other.importNode(pi);
  result.import = [imported.target, imported.data, imported.ownerDocument === other];
  const adopted = other.adoptNode(pi);
  result.adopt = [
    adopted === pi,
    pi.ownerDocument === other,
    pi.parentNode === null,
    pi.target,
    pi.data,
  ];
  doc.xmlStandalone = true;
  doc.xmlVersion = "1.0";
  result.metadataMutation = [
    doc.xmlStandalone,
    doc.xmlVersion,
    error(() => {
      doc.xmlVersion = "1.1";
    }),
    doc.xmlVersion,
  ];
  const conversion = doc.createProcessingInstruction("convert", "data");
  conversion.nodeValue = null;
  result.conversion = [
    conversion.data,
    error(() => {
      conversion.data = Symbol();
    }),
    error(() => {
      conversion.nodeValue = Symbol();
    }),
  ];
  doc.body.nodeValue = "ignored";
  result.elementNodeValue = [doc.body.nodeValue, doc.body.childNodes.length];
  const child = doc.createProcessingInstruction("child", "data");
  doc.body.appendChild(child);
  child.before("before");
  child.after("after");
  child.remove();
  result.childMethods = [child.parentNode, doc.body.textContent];
  return JSON.stringify(result);
})();
