// Shared authored localhost collector. All fixtures are removed before return.
(async () => {
  const safe = (read) => {
    try {
      const value = read();
      return value === undefined ? { undefined: true } : value;
    } catch (error) {
      return { error: error.name };
    }
  };
  const result = { text: {}, setters: {}, popover: {} };
  const fixtures = {
    inline: " A  <span>B\t C</span> D ",
    blocks: "first<div> second </div><div>third</div>end",
    paragraphs: "first<p> second </p><p>third</p>end",
    breaks: "first<br>second<br><br>",
    pre: "<pre> A  B\n C\tD </pre>end",
    hiddenChildren: "first<span hidden>hidden</span><span style='display:none'>gone</span>end",
    invisible:
      "first<span style='visibility:hidden'>invisible<span style='visibility:visible'>visible</span></span>end",
    displayContents: "first<div style='display:contents'>second<br>third</div>end",
    whitespace: "first<span style='white-space:pre'>  A\n B </span>end",
    table:
      "<table><tbody><tr><td> A </td><td> B </td></tr><tr><td>C</td><td>D</td></tr></tbody></table>end",
    textTransform: "<span style='text-transform:uppercase'>mixed Case</span>",
    replaced:
      "first<input value='input'><textarea>textarea</textarea><select><option>one</option><option>two</option></select>end",
    details: "<details><summary>summary</summary>closed</details>end",
    scriptStyle: "first<script>ignored</script><style>ignored {color:red}</style>end",
    softWrap: "<span style='display:inline-block;width:20px'>alpha beta gamma</span>",
  };
  for (const [name, html] of Object.entries(fixtures)) {
    const element = document.createElement("div");
    element.innerHTML = html;
    document.body.append(element);
    result.text[name] = safe(() => element.innerText);
    element.remove();
  }
  for (const [name, style, connected] of [
    ["disconnected", "", false],
    ["hiddenRoot", "display:none", true],
    ["hiddenAncestor", "", true],
    ["invisibleRoot", "visibility:hidden", true],
  ]) {
    const parent = document.createElement("section"),
      element = document.createElement("div");
    element.innerHTML = " A  <span hidden>hidden</span><br>B ";
    element.setAttribute("style", style);
    parent.append(element);
    if (name === "hiddenAncestor") parent.style.display = "none";
    if (connected) document.body.append(parent);
    result.text[name] = safe(() => element.innerText);
    parent.remove();
  }
  const element = document.createElement("div");
  document.body.append(element);
  for (const [name, value] of [
    ["newlines", "a\r\nb\rc\n"],
    ["markup", "<b>literal</b>"],
    ["null", null],
    ["undefined", undefined],
    ["symbol", Symbol("x")],
  ]) {
    result.setters[name] = safe(() => {
      element.innerText = value;
      return { html: element.innerHTML, text: element.textContent, inner: element.innerText };
    });
  }
  element.remove();
  result.setters.detachedOuter = safe(() => {
    element.outerText = "replacement";
    return element.outerHTML;
  });
  const parent = document.createElement("div");
  parent.innerHTML = "left<span id='replace'>old</span>right";
  document.body.append(parent);
  result.setters.outer = safe(() => {
    const target = parent.querySelector("span");
    target.outerText = "A\nB";
    return {
      html: parent.innerHTML,
      children: [...parent.childNodes].map((node) => [node.nodeType, node.nodeValue]),
      detached: target.parentNode === null,
    };
  });
  parent.remove();
  const popup = document.createElement("div");
  document.body.append(popup);
  for (const value of [null, "", "auto", "manual", "hint", "AUTO", "bad", " auto "])
    result.popover[String(value)] = safe(() => {
      if (value === null) popup.removeAttribute("popover");
      else popup.setAttribute("popover", value);
      return popup.popover;
    });
  result.popover.methods = {};
  for (const [name, operation] of Object.entries({
    missingShow: () => {
      popup.removeAttribute("popover");
      return popup.showPopover();
    },
    disconnectedShow: () => {
      popup.popover = "manual";
      popup.remove();
      return popup.showPopover();
    },
    show: () => {
      document.body.append(popup);
      popup.popover = "manual";
      return [popup.showPopover(), popup.matches(":popover-open")];
    },
    showAgain: () => [popup.showPopover(), popup.matches(":popover-open")],
    forceOpen: () => popup.togglePopover(true),
    hide: () => [popup.hidePopover(), popup.matches(":popover-open")],
    hideAgain: () => popup.hidePopover(),
    toggle: () => [popup.togglePopover(), popup.togglePopover()],
  }))
    result.popover.methods[name] = safe(operation);
  popup.remove();
  const eventRows = [],
    automatic = [];
  for (let index = 0; index < 2; index++) {
    const element = document.createElement("div");
    element.popover = "auto";
    for (const name of ["beforetoggle", "toggle"])
      element.addEventListener(name, (event) =>
        eventRows.push([
          index,
          event.type,
          event.oldState,
          event.newState,
          event.cancelable,
          event.isTrusted,
          event instanceof ToggleEvent,
        ]),
      );
    document.body.append(element);
    automatic.push(element);
  }
  result.popover.automatic = safe(() => {
    automatic[0].showPopover();
    automatic[1].showPopover();
    return automatic.map((element) => element.matches(":popover-open"));
  });
  await new Promise((done) => setTimeout(done, 0));
  result.popover.events = eventRows;
  for (const element of automatic) element.remove();
  result.popover.eventConstructor = safe(() => {
    const event = new ToggleEvent("sample", { oldState: "old", newState: "new" });
    return [event.oldState, event.newState, event.source, event.isTrusted, event instanceof Event];
  });
  const encoded = JSON.stringify(result);
  globalThis.__RESULT = encoded;
  return encoded;
})();
