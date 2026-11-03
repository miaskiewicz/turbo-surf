(async () => {
  const get = (path, responseType, mime) =>
    new Promise((done) => {
      const x = new XMLHttpRequest();
      x.open("GET", path);
      if (responseType !== undefined) x.responseType = responseType;
      if (mime) x.overrideMimeType(mime);
      x.onloadend = () => done(x);
      x.send();
    });
  const describe = (doc) =>
    doc === null
      ? null
      : [
          Object.prototype.toString.call(doc),
          doc.contentType,
          doc.documentElement && doc.documentElement.namespaceURI,
          doc.documentElement && doc.documentElement.localName,
          new XMLSerializer().serializeToString(doc),
        ];
  const out = {};
  for (const [name, path, type, mime] of [
    ["xml", "/xml", ""],
    ["textXml", "/text-xml", ""],
    ["svg", "/svg", ""],
    ["xhtml", "/xhtml", ""],
    ["illFormed", "/bad", ""],
    ["html", "/html", ""],
    ["plain", "/plain", ""],
    ["xmlDocument", "/xml", "document"],
    ["htmlDocument", "/html", "document"],
    ["plainDocument", "/plain", "document"],
    ["override", "/plain", "", "text/xml"],
    ["latin1Declared", "/latin1-decl", ""],
    ["utf16Bom", "/utf16-bom", ""],
    ["headerLatin1", "/header-latin1", ""],
    ["bomBeatsHeader", "/bom-beats-header", ""],
    ["headerVsDecl", "/header-vs-decl", ""],
    ["literalParsererror", "/literal-parsererror", ""],
  ]) {
    const x = await get(path, type, mime);
    out[name] = [
      describe(x.responseXML),
      x.response === x.responseXML,
      type === "document" ? null : x.responseText.length,
    ];
  }
  const pending = new XMLHttpRequest();
  pending.open("GET", "/xml");
  out.unsent = pending.responseXML;
  try {
    const x = await get("/xml", "text");
    out.textType = x.responseXML;
  } catch (error) {
    out.textType = [error.name, error.message];
  }
  return out;
})();
