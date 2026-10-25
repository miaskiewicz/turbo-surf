(async () => {
  // Server echoes { method, contentType, bodyHex } for every /wire request.
  const files = () => {
    const data = new DataTransfer();
    data.items.add(
      new File(["hello\r\n"], 'a "q"\r\n%.txt', { type: "text/plain", lastModified: 1 }),
    );
    data.items.add(
      new File([new Uint8Array([0, 255, 10, 13, 128])], "b\nc\rd.bin", { lastModified: 2 }),
    );
    data.items.add(new File(["é"], "ü€😀.txt", { type: "Text/X; charset=UTF-8", lastModified: 3 }));
    return data.files;
  };
  const build = () => {
    const form = document.createElement("form");
    form.method = "post";
    form.enctype = "multipart/form-data";
    const add = (tag, name, type) => {
      const control = document.createElement(tag);
      if (type) control.type = type;
      if (name !== null) control.setAttribute("name", name);
      form.append(control);
      return control;
    };
    add("input", "txt").value = 'aé€😀 "q"\r\nx\ny\rz';
    add("input", "files", "file").files = files();
    add("input", "empty", "file");
    add("input", 'n"a\r\nm\ne\ré€', "file").files = files();
    add("textarea", "ta").value = "l1\nl2\rl3";
    add("input", "c", "checkbox").checked = true;
    add("input", "_charset_", "hidden");
    add("input", "disabled", "file").disabled = true;
    add("input", null, "file").files = files();
    document.body.append(form);
    return form;
  };
  const post = async (kind, body) =>
    (await fetch(`/wire?kind=${kind}`, { method: "POST", body })).json();
  const result = {};
  const data = new FormData();
  data.append('t"x\r\ny\nz\r%é', "v\nline\r\nz\r");
  data.append("file", files()[0]);
  data.append("blob", new Blob(["B"]));
  data.append("named", new Blob(["N"], { type: "image/png" }), 'n\n"\r.png');
  data.append("é€", "ü€");
  result.fetch = await post("fetch", data);
  result.formData = await post("form-data", new FormData(build()));
  const form = build();
  form.action = "/wire?kind=submit";
  if (globalThis.__wireSelf) {
    form.submit();
    return result;
  }
  const frame = document.createElement("iframe");
  frame.name = "wire";
  document.body.append(frame);
  form.target = "wire";
  const loaded = new Promise((done) => frame.addEventListener("load", done, { once: true }));
  form.submit();
  await loaded;
  result.submit = JSON.parse(frame.contentDocument.body.textContent);
  return result;
})();
