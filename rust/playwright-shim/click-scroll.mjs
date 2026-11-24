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
  const view = element.ownerDocument.defaultView;
  const clipBounds = () => {
    const clip = { left: 0, top: 0, right: view.innerWidth, bottom: view.innerHeight };
    for (
      let parent = __tsNs.tcLocatorParent(element);
      parent;
      parent = __tsNs.tcLocatorParent(parent)
    ) {
      const style = view.getComputedStyle(parent);
      const x = /^(auto|scroll|hidden|clip)$/.test(style.overflowX);
      const y = /^(auto|scroll|hidden|clip)$/.test(style.overflowY);
      if (!x && !y) continue;
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
