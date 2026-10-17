(async () => {
  const result = [];
  const cases = [
    [
      "get",
      "application/x-www-form-urlencoded",
      [
        ["a b", "x+y"],
        ["dup", "1"],
        ["dup", "2"],
        ["line\rname", "a\nb\rc\r\nd"],
        ["unicode", "é😀"],
      ],
    ],
    ["get", "text/plain", []],
    [
      "post",
      "application/x-www-form-urlencoded",
      [
        ["a&=", "x y+"],
        ["line", "\r\n\n\r"],
      ],
    ],
    [
      "post",
      "text/plain",
      [
        ["a=b", "hello\nworld"],
        ["unicode", "é😀"],
      ],
    ],
  ];
  for (const [method, enctype, entries] of cases) {
    const frame = document.createElement("iframe");
    frame.name = `wire-${result.length}`;
    document.body.append(frame);
    const form = document.createElement("form");
    form.method = method;
    form.enctype = enctype;
    form.target = frame.name;
    form.action = `${location.origin}/wire?old=1#retained`;
    for (const [name, value] of entries) {
      const input = document.createElement("input");
      input.type = "hidden";
      input.name = name;
      input.value = value;
      form.append(input);
    }
    document.body.append(form);
    const done = new Promise((resolve) => frame.addEventListener("load", resolve, { once: true }));
    form.requestSubmit();
    await done;
    const wire = JSON.parse(frame.contentDocument.body.textContent);
    result.push({ method, enctype, entries, wire, fragment: frame.contentWindow.location.hash });
    form.remove();
    frame.remove();
  }
  return result;
})();
