(async () => {
  // turbo-surf#32 item 2: a parser-inserted image loads during parsing, before a script
  // that runs over the parsed document (render()'s script, an inline script at the end of
  // the body in Chrome) reads it; its load event still fires after that script.
  const image = document.getElementById("parsed");
  let event = "none";
  const early = [image.complete, image.naturalWidth, image.naturalHeight];
  image.addEventListener("load", () => (event = "load"));
  image.addEventListener("error", () => (event = "error"));
  await new Promise((done) => setTimeout(done, 1500));
  const late = [image.complete, image.naturalWidth, image.naturalHeight, event];
  document.title = JSON.stringify({ early, late, own: Object.getOwnPropertyNames(image) });
})();
