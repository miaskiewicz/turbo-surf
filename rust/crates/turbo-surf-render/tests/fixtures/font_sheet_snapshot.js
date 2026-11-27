export function fontSheetSnapshotProbe(poison) {
  const descriptor = Object.getOwnPropertyDescriptor(StyleSheetList.prototype, "length");
  let reads = 0;
  Object.defineProperty(StyleSheetList.prototype, "length", {
    configurable: true,
    get() {
      reads++;
      if (poison) throw new Error("authored stylesheet-list getter");
      return Reflect.apply(descriptor.get, this, []);
    },
  });
  for (let index = 0; index < 64; index++) {
    const style = document.createElement("style");
    style.textContent = `.unused${index}{color:blue}`;
    document.head.appendChild(style);
  }
  const addFace = (family) => {
    const style = document.createElement("style");
    style.textContent = `@font-face{font-family:${family};src:url(data:font/woff;base64,AA==)}`;
    document.head.appendChild(style);
    return style;
  };
  const first = addFace("SheetProbeAlpha");
  const sizes = [document.fonts.size];
  const firstReads = reads;
  const second = addFace("SheetProbeBeta");
  sizes.push(document.fonts.size);
  second.remove();
  sizes.push(document.fonts.size);
  first.remove();
  sizes.push(document.fonts.size);
  return { firstReads, sizes };
}
