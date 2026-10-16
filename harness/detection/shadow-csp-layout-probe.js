(() => {
  const policy = document.createElement("meta");
  policy.httpEquiv = "Content-Security-Policy";
  policy.content = "style-src 'nonce-layout-control'";
  document.head.append(policy);
  const style = document.createElement("style");
  style.nonce = "layout-control";
  style.textContent = ".target{display:block;width:40px;height:20px}";
  document.head.append(style);
  const light = document.createElement("div");
  light.className = "target";
  light.setAttribute("style", "width:123px;height:77px");
  document.body.append(light);
  const host = document.createElement("div");
  document.body.append(host);
  const root = host.attachShadow({ mode: "closed" });
  root.innerHTML =
    '<style nonce="layout-control">.target{display:block;width:40px;height:20px}</style><div class="target" style="width:123px;height:77px"></div>';
  const shadow = root.querySelector(".target");
  const size = (node) => {
    const rect = node.getBoundingClientRect();
    return [rect.width, rect.height];
  };
  return {
    light: size(light),
    shadow: size(shadow),
    raw: [light.getAttribute("style"), shadow.getAttribute("style")],
  };
})();
