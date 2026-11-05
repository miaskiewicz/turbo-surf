(async () => {
  // Body#formData(): multipart/form-data and URL-encoded parsing, the MIME and
  // body checks that decide between "Invalid MIME type" and "Failed to fetch",
  // whether a failed read consumes the body, and the File entries it builds.
  const out = {};
  const encode = (text) => new TextEncoder().encode(text);
  const join = (...parts) => {
    const bytes = [];
    for (const part of parts) bytes.push(...(typeof part === "string" ? encode(part) : part));
    return new Uint8Array(bytes);
  };
  const codes = (text) => Array.from(text, (character) => character.codePointAt(0));
  const entries = async (form) => {
    const rows = [];
    for (const [name, value] of form)
      rows.push([
        codes(name),
        typeof value === "string"
          ? codes(value)
          : [
              Object.getPrototypeOf(value) === File.prototype,
              value.name,
              value.type,
              value.size,
              codes(await value.text()),
              typeof value.lastModified,
              Math.abs(value.lastModified - Date.now()) < 60000,
            ],
      ]);
    return rows;
  };
  const read = async (body, type, Kind = "response") => {
    const headers = type === null ? {} : { "content-type": type };
    const target =
      Kind === "request"
        ? new Request("http://127.0.0.1/", { method: "POST", body, headers })
        : new Response(body, { headers });
    try {
      return ["ok", await entries(await target.formData()), target.bodyUsed];
    } catch (error) {
      return [error.constructor.name, error.message, target.bodyUsed];
    }
  };
  const H = 'Content-Disposition: form-data; name="a"';
  const M = "multipart/form-data; boundary=x";
  const parts = {
    basic: `--x\r\n${H}\r\n\r\n1\r\n--x\r\nContent-Disposition: form-data; name="f"; filename="x.txt"\r\nContent-Type: text/plain\r\n\r\nhi\r\n--x--\r\n`,
    fileNoType: `--x\r\nContent-Disposition: form-data; name="f"; filename="a.bin"\r\n\r\nzz\r\n--x--`,
    fileRawType: `--x\r\nContent-Disposition: form-data; name="f"; filename="p/q\\\\r.txt"\r\nContent-Type: Image/PNG\r\n\r\nv\r\n--x--`,
    fileTypeParams: `--x\r\nContent-Disposition: form-data; name="f"; filename="z"\r\nContent-Type:  text/plain; charset=UTF-8 \r\n\r\nv\r\n--x--`,
    fileBlankType: `--x\r\n${H}; filename="f"\r\nContent-Type: \r\n\r\nv\r\n--x--`,
    fileEmptyName: `--x\r\nContent-Disposition: form-data; name="a\\"b"; filename=""\r\n\r\nv\r\n--x--`,
    fileFirstType: `--x\r\nContent-Type: a/b\r\nContent-Disposition: form-data; name="f"; filename="z"\r\nContent-Type: c/d\r\n\r\nv\r\n--x--`,
    fileNameFirst: `--x\r\nContent-Disposition: form-data; filename="z"; name="f"\r\n\r\nv\r\n--x--`,
    fileUtf8: `--x\r\nContent-Disposition: form-data; name="f"; filename="ż.txt"\r\n\r\nv\r\n--x--`,
    filePercent: `--x\r\nContent-Disposition: form-data; name="f"; filename="%C5%BC"\r\n\r\nv\r\n--x--`,
    fileBadName: join(
      '--x\r\nContent-Disposition: form-data; name="f"; filename="',
      [0xff],
      '"\r\n\r\nv\r\n--x--',
    ),
    fileRawBytes: join(
      '--x\r\nContent-Disposition: form-data; name="f"; filename="b"\r\n\r\n',
      [0xff, 0],
      "\r\n--x--",
    ),
    preamble: `junk\r\n--x\r\n${H}\r\n\r\nv\r\n--x--\r\nepilogue`,
    leadingCrlf: `\r\n--x\r\n${H}\r\n\r\nv\r\n--x--`,
    lfOnly: `--x\n${H}\n\nv\n--x--`,
    lfBeforeClose: `--x\r\n${H}\r\n\r\nv\n--x--`,
    noName: "--x\r\nContent-Disposition: form-data\r\n\r\nv\r\n--x--",
    noDisposition: "--x\r\nContent-Type: text/plain\r\n\r\nv\r\n--x--",
    noHeaders: "--x\r\n\r\nv\r\n--x--",
    blankHeaderLine: "--x\r\n\r\n\r\nv\r\n--x--",
    noClose: `--x\r\n${H}\r\n\r\nv\r\n--x`,
    empty: "",
    onlyClose: "--x--",
    closeFirst: `--x--\r\n--x\r\n${H}\r\n\r\nv\r\n--x--`,
    utf8: `--x\r\nContent-Disposition: form-data; name="ż"\r\n\r\nł\r\n--x--`,
    badUtf8Value: join(`--x\r\n${H}\r\n\r\n`, [0xff, 0xfe], "\r\n--x--"),
    badUtf8Name: join(
      '--x\r\nContent-Disposition: form-data; name="',
      [0xff],
      '"\r\n\r\nv\r\n--x--',
    ),
    bomValue: join(`--x\r\n${H}\r\n\r\n`, [0xef, 0xbb, 0xbf], "v\r\n--x--"),
    bomName: join(
      '--x\r\nContent-Disposition: form-data; name="',
      [0xef, 0xbb, 0xbf],
      'a"\r\n\r\nv\r\n--x--',
    ),
    nulValue: `--x\r\n${H}\r\n\r\nv\0w\r\n--x--`,
    crlfValue: `--x\r\n${H}\r\n\r\na\r\nb\nc\r\n--x--`,
    emptyValue: `--x\r\n${H}\r\n\r\n\r\n--x--`,
    twoParts: `--x\r\n${H}\r\n\r\n1\r\n--x\r\n${H}\r\n\r\n2\r\n--x--`,
    percentName: '--x\r\nContent-Disposition: form-data; name="a%22b"\r\n\r\nv\r\n--x--',
    escapes: '--x\r\nContent-Disposition: form-data; name="a\\b\\\\c"\r\n\r\nv\r\n--x--',
    unquoted: "--x\r\nContent-Disposition: form-data; name=a\r\n\r\nv\r\n--x--",
    singleQuotes: "--x\r\nContent-Disposition: form-data; name='a'\r\n\r\nv\r\n--x--",
    commaValue: "--x\r\nContent-Disposition: form-data; name=a,b\r\n\r\nv\r\n--x--",
    nonAsciiToken: "--x\r\nContent-Disposition: form-data; name=ż\r\n\r\nv\r\n--x--",
    lowerHeader: '--x\r\ncontent-disposition: form-data; name="a"\r\n\r\nv\r\n--x--',
    upperType: '--x\r\nContent-Disposition: FORM-DATA; name="a"\r\n\r\nv\r\n--x--',
    upperParam: '--x\r\nContent-Disposition: form-data; NAME="a"\r\n\r\nv\r\n--x--',
    inline: '--x\r\nContent-Disposition: inline; name="a"\r\n\r\nv\r\n--x--',
    tightParams: '--x\r\nContent-Disposition:form-data;name="a"\r\n\r\nv\r\n--x--',
    looseParams: '--x\r\nContent-Disposition:   form-data  ;  name = "a"  \r\n\r\nv\r\n--x--',
    tabs: '--x\r\nContent-Disposition:\tform-data;\tname="a"\r\n\r\nv\r\n--x--',
    trailingSemicolon: '--x\r\nContent-Disposition: form-data; name="a";\r\n\r\nv\r\n--x--',
    duplicateParam: `--x\r\n${H}; name="b"\r\n\r\nv\r\n--x--`,
    duplicateHeader: `--x\r\n${H}\r\nContent-Disposition: form-data; name="b"\r\n\r\nv\r\n--x--`,
    extendedParam: `--x\r\n${H}; filename*=UTF-8''a.txt\r\n\r\nv\r\n--x--`,
    typeWithoutFile: `--x\r\n${H}\r\nContent-Type: image/png\r\n\r\nv\r\n--x--`,
    extraHeader: `--x\r\n${H}\r\nX-Foo: bar\r\n\r\nv\r\n--x--`,
    colonless: `--x\r\n${H}\r\nnocolon\r\n\r\nv\r\n--x--`,
    emptyHeaderName: `--x\r\n: x\r\n${H}\r\n\r\nv\r\n--x--`,
    spaceBeforeColon: '--x\r\nContent-Disposition :form-data; name="a"\r\n\r\nv\r\n--x--',
    folded: '--x\r\nContent-Disposition: form-data;\r\n name="a"\r\n\r\nv\r\n--x--',
    lfHeader: `--x\r\n${H}\nX: y\r\n\r\nv\r\n--x--`,
    innerDelimiter: `--x\r\n${H}\r\n\r\nv--x\r\n--xy\r\n--x--`,
    paddedDelimiter: `--x \t\r\n${H}\r\n\r\nv\r\n--x--`,
    paddedClose: `--x\r\n${H}\r\n\r\nv\r\n--x-- \t`,
    closePaddingCrlf: `--x\r\n${H}\r\n\r\nv\r\n--x--  \r\n`,
    closeJunk: `--x\r\n${H}\r\n\r\nv\r\n--x--junk`,
  };
  out.parts = {};
  for (const [name, body] of Object.entries(parts)) out.parts[name] = await read(body, M);
  const simple = `--x\r\n${H}\r\n\r\nv\r\n--x--`;
  const types = [
    "MULTIPART/FORM-DATA; BOUNDARY=x",
    "Multipart/Form-Data; boundary=x",
    'multipart/form-data; Boundary="x"',
    "multipart/form-data;boundary=x;charset=utf-8",
    "multipart/form-data;  boundary = x",
    "multipart/form-data ; boundary=x",
    'multipart/form-data;boundary="x";x=1',
    "multipart/form-data; boundary=X",
    'multipart/form-data; boundary="x"; boundary=y',
    'multipart/form-data; boundary="x\\"y"',
    "multipart/form-data; boundary=",
    'multipart/form-data; boundary=""',
    "multipart/form-data; boundary=x y",
    'multipart/form-data; boundary=a"b',
    "multipart/form-data; boundary=x;",
    "multipart/form-data;=;boundary=x",
    "multipart/form-data; foo; boundary=x",
    "multipart/form-data, text/plain; boundary=x",
    "multipart/form-datax; boundary=x",
    "multipart/form-data",
  ];
  out.types = {};
  for (const type of types) out.types[type] = await read(simple, type);
  const urlencoded = [
    "application/x-www-form-urlencoded",
    "application/x-www-form-urlencoded;charset=x",
    "APPLICATION/X-WWW-FORM-URLENCODED",
    " application/x-www-form-urlencoded ",
    "text/plain",
    null,
  ];
  out.urlencoded = {};
  for (const type of urlencoded) out.urlencoded[String(type)] = await read("a=1&b=%20+&c", type);
  out.urlencoded.badUtf8 = await read(
    new Uint8Array([0x61, 0x3d, 0xff]),
    "application/x-www-form-urlencoded",
  );
  out.urlencoded.bom = await read("﻿a=1", "application/x-www-form-urlencoded");
  out.nullBodies = {};
  for (const type of [
    null,
    "text/plain",
    M,
    "multipart/form-data",
    "application/x-www-form-urlencoded",
  ])
    for (const body of [null, ""])
      out.nullBodies[`${type} ${body === null ? "null" : "empty"}`] = await read(body, type);
  out.request = {
    multipart: await read(simple, M, "request"),
    textPlain: await read("a", "text/plain", "request"),
    noBoundary: await read("a", "multipart/form-data", "request"),
    nullBody: await (async () => {
      try {
        return ["ok", await entries(await new Request("http://127.0.0.1/").formData())];
      } catch (error) {
        return [error.constructor.name, error.message];
      }
    })(),
  };
  const fd = new FormData();
  fd.append("a", "1");
  fd.append("f", new Blob(["zz"], { type: "x/y" }), "n.txt");
  out.roundTrip = await (async () => {
    const response = new Response(fd);
    return [
      response.headers.get("content-type").split(";")[0],
      await entries(await response.formData()),
    ];
  })();
  // A patched FormData.prototype.append or TextDecoder must not reach the parse.
  const append = FormData.prototype.append,
    Decoder = globalThis.TextDecoder;
  let hooked = 0;
  FormData.prototype.append = function (...args) {
    hooked++;
    return Reflect.apply(append, this, args);
  };
  globalThis.TextDecoder = function () {
    hooked++;
    throw new Error("patched");
  };
  try {
    out.hooks = [
      await read(parts.basic, M),
      await read("a=1", "application/x-www-form-urlencoded"),
      hooked,
    ];
  } finally {
    FormData.prototype.append = append;
    globalThis.TextDecoder = Decoder;
  }
  return JSON.stringify(out);
})();
