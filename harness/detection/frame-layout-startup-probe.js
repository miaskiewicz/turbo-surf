(async () => {
  const style = document.createElement("style");
  style.textContent =
    "@font-face{font-family:UnloadedLayoutFace;src:url(/not-loaded.woff2)}body{font-family:UnloadedLayoutFace}";
  document.head.append(style);
  const frame = document.createElement("iframe");
  frame.style = "width:200px;height:100px;border:0";
  const loaded = new Promise((resolve) => frame.addEventListener("load", resolve, { once: true }));
  frame.srcdoc = "<!doctype html><body><script>globalThis.startedByPageScript=true</script>";
  document.body.append(frame);
  await loaded;
  let initial;
  try {
    initial = { width: frame.contentWindow.innerWidth };
  } catch (error) {
    initial = { error: error.message };
  }
  const started = frame.contentWindow.startedByPageScript;
  style.remove();
  const widthAfterClear = frame.contentWindow.innerWidth;
  frame.remove();
  return { started, initial, widthAfterClear };
})();
