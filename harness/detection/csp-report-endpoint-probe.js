(item) => {
  globalThis.reportEventCount = 0;
  document.addEventListener("securitypolicyviolation", () => {
    reportEventCount++;
  });
  if (item.tamper) {
    Object.prototype.toJSON = () => "guest-toJSON";
    globalThis.JSON.stringify = () => "guest-stringify";
    globalThis.fetch = () => {
      throw new Error("guest-fetch");
    };
  }
  if (item.meta) {
    const meta = document.createElement("meta");
    meta.httpEquiv = "Content-Security-Policy";
    meta.content = item.meta;
    document.head.append(meta);
  }
  if (item.request) {
    fetch("/allowed?case=" + item.id).catch(() => {});
    return document.referrer;
  }
  const sample = item.sampleUnits ? String.fromCharCode(...item.sampleUnits) : "color:red";
  document.body.setAttribute("style", sample);
  return document.referrer;
};
