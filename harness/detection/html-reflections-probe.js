(() => {
  const rows = {};
  for (const tag of [
    "div",
    "input",
    "button",
    "a",
    "select",
    "textarea",
    "iframe",
    "object",
    "summary",
    "audio",
    "video",
  ]) {
    const element = document.createElement(tag);
    const values = [element.tabIndex];
    if (tag === "a") element.setAttribute("href", "/");
    if (tag === "audio" || tag === "video") element.setAttribute("controls", "");
    values.push(element.tabIndex);
    for (const value of ["", "bad", "7suffix", "  -2junk", "2147483648", "-2147483649"]) {
      element.setAttribute("tabindex", value);
      values.push(element.tabIndex);
    }
    element.tabIndex = 4294967297;
    values.push([element.tabIndex, element.getAttribute("tabindex")]);
    rows[tag] = values;
  }
  const div = document.createElement("div");
  div.setAttribute("contenteditable", "true");
  rows.editable = div.tabIndex;
  const input = document.createElement("input");
  input.type = "hidden";
  rows.hiddenInput = input.tabIndex;
  const nonces = [];
  for (const tag of ["div", "script", "style"]) {
    const element = document.createElement(tag);
    element.nonce = "token";
    nonces.push([tag, element.nonce, element.getAttribute("nonce")]);
    document.body.appendChild(element);
    nonces.push([tag, element.nonce, element.getAttribute("nonce")]);
    element.setAttribute("nonce", "changed");
    nonces.push([tag, element.nonce, element.getAttribute("nonce")]);
    element.remove();
  }
  rows.nonces = nonces;
  return JSON.stringify(rows);
})();
