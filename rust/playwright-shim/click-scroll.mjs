function captureClickScroll() {
  if (__tsNs.shimNativeClickScroll) return;
  const apply = Reflect.apply;
  const scroll = Element.prototype.scrollIntoViewIfNeeded;
  __tsNs.shimNativeClickScroll = (element) => {
    if (typeof scroll !== "function") throw new Error("Native click scrolling is unavailable");
    apply(scroll, element, []);
  };
}

export const clickScrollBootstrap = `(${captureClickScroll.toString()})();`;

export function scrollClickTarget(element) {
  const document = element.ownerDocument;
  const view = document.defaultView;
  const permitsBodyPropagation = (style) =>
    style.display !== "none" &&
    ["", "none"].includes(style.contain) &&
    !["auto", "hidden"].includes(style.contentVisibility) &&
    !/(?:^|\s)(?:size|inline-size)(?:\s|$)/.test(style.containerType);
  const bodyOverflowIsViewport = (body, style) => {
    const root = document.documentElement;
    if (
      body !== document.body ||
      __tsNs.tcLocatorParent(body) !== root ||
      root?.namespaceURI !== "http://www.w3.org/1999/xhtml" ||
      root.localName !== "html" ||
      body.localName !== "body" ||
      !permitsBodyPropagation(style)
    )
      return false;
    const rootStyle = view.getComputedStyle(root);
    return (
      rootStyle.overflowX === "visible" &&
      rootStyle.overflowY === "visible" &&
      permitsBodyPropagation(rootStyle)
    );
  };
  const fixedContainingBlock = (style) =>
    ["transform", "translate", "rotate", "scale", "perspective", "filter", "backdropFilter"].some(
      (name) => style[name] && style[name] !== "none",
    ) ||
    /(?:^|\s)(?:layout|paint|strict|content)(?:\s|$)/.test(style.contain) ||
    /(?:^|[,\s])(?:transform|perspective|filter|backdrop-filter|contain)(?:[,\s]|$)/.test(
      style.willChange,
    ) ||
    ["auto", "hidden"].includes(style.contentVisibility);
  const clipBounds = () => {
    let fixedEscapes = view.getComputedStyle(element).position === "fixed";
    const clip = { left: 0, top: 0, right: view.innerWidth, bottom: view.innerHeight };
    for (
      let parent = __tsNs.tcLocatorParent(element);
      parent && parent !== element.ownerDocument.documentElement;
      parent = __tsNs.tcLocatorParent(parent)
    ) {
      const style = view.getComputedStyle(parent);
      if (fixedEscapes && fixedContainingBlock(style)) fixedEscapes = false;
      const applyClip = !fixedEscapes;
      if (style.position === "fixed") fixedEscapes = true;
      if (!applyClip) continue;
      const x = /^(auto|scroll|hidden|clip)$/.test(style.overflowX);
      const y = /^(auto|scroll|hidden|clip)$/.test(style.overflowY);
      if (!x && !y) continue;
      // CSS Overflow 3 viewport propagation: the used body overflow is visible.
      if (bodyOverflowIsViewport(parent, style)) continue;
      const box = parent.getBoundingClientRect();
      const left = box.left + parent.clientLeft;
      const top = box.top + parent.clientTop;
      if (x) {
        clip.left = Math.max(clip.left, left);
        clip.right = Math.min(clip.right, left + parent.clientWidth);
      }
      if (y) {
        clip.top = Math.max(clip.top, top);
        clip.bottom = Math.min(clip.bottom, top + parent.clientHeight);
      }
    }
    return clip;
  };
  let rect = element.getBoundingClientRect();
  let clip = clipBounds();
  if (
    rect.left >= clip.left &&
    rect.top >= clip.top &&
    rect.right <= clip.right &&
    rect.bottom <= clip.bottom
  )
    return { rect, clip };
  __tsNs.shimNativeClickScroll(element);
  if (!element.isConnected) throw new Error("Click target detached during scrolling");
  rect = element.getBoundingClientRect();
  clip = clipBounds();
  return { rect, clip };
}
