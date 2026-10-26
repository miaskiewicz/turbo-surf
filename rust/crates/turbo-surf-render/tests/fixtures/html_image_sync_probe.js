(() => {
  "use strict";
  // turbo-surf#38: Chrome loads a data: image synchronously — complete, with its natural size
  // (0x0 when broken), as soon as its source is set: for parser images at the first script
  // read (`window.early`, recorded by the page's inline script) and for script-created,
  // fragment-parsed and connected images right after assigning src. Rows:
  // [complete, naturalWidth, naturalHeight].
  const png = document.getElementById("png").getAttribute("src");
  const svg = document.getElementById("svg").getAttribute("src");
  const row = (img) => [img.complete, img.naturalWidth, img.naturalHeight];
  const out = { early: window.early };
  const created = (src) => {
    const img = new Image();
    img.src = src;
    return row(img);
  };
  out.created = created(png);
  out.createdSvg = created(svg);
  out.createdBroken = created("data:image/png;base64,AAAA");
  const holder = document.createElement("div");
  holder.innerHTML = '<img src="' + png + '">';
  out.innerHTML = row(holder.firstChild);
  const connected = document.createElement("img");
  document.body.append(connected);
  connected.setAttribute("src", svg);
  out.connected = row(connected);
  const pending = new Image();
  pending.src = "https://html-image.test/never.png";
  out.network = [pending.complete, pending.naturalWidth];
  return out;
})();
