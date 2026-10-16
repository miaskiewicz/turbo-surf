(() => {
  const selectors = [
    "main:has(> .x)",
    "main:has(> .missing)",
    "main:has(> .x + .y)",
    "main > .x:has(+ .y)",
    "main > :is(main > .x)",
    "main > :where(.y)",
    "main > [data-token='{,}']",
    "main > .x, main > .y",
  ];
  function measure(root) {
    const main = document.createElement("main");
    main.innerHTML = "<div class='x' data-token='{,}'></div><div class='y'></div>";
    root.append(main);
    const style = document.createElement("style");
    root.append(style);
    const results = selectors.map((selector) => {
      style.textContent = `main,main > div{width:11px;height:7px}${selector}{width:43px}`;
      return {
        widths: [main, ...main.children].map((node) => node.getBoundingClientRect().width),
        matches: Array.from(
          root.querySelectorAll(selector),
          (node) => node.className || node.localName,
        ),
      };
    });
    style.remove();
    main.remove();
    return results;
  }
  const light = measure(document.body);
  const host = document.createElement("div");
  document.body.append(host);
  const shadow = measure(host.attachShadow({ mode: "closed" }));
  host.remove();
  return { light, shadow };
})();
