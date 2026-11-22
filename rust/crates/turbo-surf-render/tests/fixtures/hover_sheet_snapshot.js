export function hoverSheetSnapshotProbe(poison) {
  for (let index = 0; index < 64; index++) {
    const style = document.createElement("style");
    document.head.appendChild(style);
    style.sheet.insertRule(`.unused${index}{color:blue}`);
  }
  const style = document.createElement("style");
  document.head.appendChild(style);
  const sheet = style.sheet;
  sheet.insertRule("#m:hover #d{visibility:visible}");
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
  const menu = document.getElementById("m");
  const item = document.getElementById("d");
  __tsNs.tcApplyHover(menu);
  const firstReads = reads;
  const states = [item.style.visibility];
  sheet.deleteRule(0);
  sheet.insertRule("#m:hover #d{visibility:hidden}");
  __tsNs.tcApplyHover(menu);
  states.push(item.style.visibility);
  return { reads: firstReads, states };
}
