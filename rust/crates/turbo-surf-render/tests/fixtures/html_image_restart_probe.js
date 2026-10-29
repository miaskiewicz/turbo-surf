(async () => {
  "use strict";
  // turbo-surf#49: every relevant mutation restarts an image's load (HTML "update the image
  // data") and fires load again, even when the selected source stays the same: setting src to
  // its own value, setting an <img>'s srcset while a <source> wins, removing a <source> that
  // did not win. Moving a loaded image is not a relevant mutation. Each row records whether
  // a load / error event fired within the cap and the image's state after it.
  const img = document.getElementById("img");
  const plain = document.getElementById("plain");
  const step = (target, mutate) =>
    new Promise((done) => {
      const events = [];
      const finish = (event) => {
        events.push(event.type);
        clearTimeout(timer);
        target.removeEventListener("load", finish);
        target.removeEventListener("error", finish);
        done([events, new URL(target.currentSrc).pathname, target.naturalWidth]);
      };
      // A cap, not a timing: a restarted local load fires within milliseconds.
      const timer = setTimeout(() => {
        target.removeEventListener("load", finish);
        target.removeEventListener("error", finish);
        done([events, new URL(target.currentSrc).pathname, target.naturalWidth]);
      }, 1500);
      target.addEventListener("load", finish);
      target.addEventListener("error", finish);
      mutate();
    });
  const out = {};
  out.sameSrc = await step(plain, () => plain.setAttribute("src", plain.getAttribute("src")));
  out.moved = await step(plain, () => document.body.append(plain));
  out.removeLosingSource = await step(img, () => document.getElementById("losing").remove());
  out.imageSrcsetUnderSource = await step(img, () =>
    img.setAttribute("srcset", "/img/90x45/own.png"),
  );
  out.unrelated = await step(img, () => img.setAttribute("alt", "x"));
  return out;
})();
