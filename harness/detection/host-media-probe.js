(() => {
  const queries = [
    "(forced-colors)",
    "(forced-colors: active)",
    "(forced-colors: none)",
    "not (forced-colors: active)",
    "(forced-colors: invalid)",
    "(prefers-contrast)",
    "(prefers-contrast: no-preference)",
    "(prefers-contrast: more)",
    "(prefers-contrast: less)",
    "(prefers-contrast: custom)",
    "(prefers-color-scheme)",
    "(prefers-color-scheme: dark)",
    "(prefers-color-scheme: light)",
    "(prefers-reduced-motion)",
    "(prefers-reduced-motion: reduce)",
    "(prefers-reduced-motion: no-preference)",
    "(prefers-reduced-transparency)",
    "(prefers-reduced-transparency: reduce)",
    "(prefers-reduced-transparency: no-preference)",
    "(hover)",
    "(hover: hover)",
    "(hover: none)",
    "(pointer)",
    "(pointer: fine)",
    "(pointer: coarse)",
    "(pointer: none)",
    "(any-hover: hover)",
    "(any-pointer: fine)",
    "(forced-colors: active), (prefers-contrast: more)",
    "(forced-colors: none) and (min-width: 1px)",
    "not ((forced-colors: active) and (prefers-color-scheme: dark))",
  ];
  const matches = queries.map((query) => [query, matchMedia(query).matches]);
  const style = document.createElement("style");
  style.textContent =
    "#host-media{width:10px;height:10px}@media(forced-colors:none){#host-media{width:20px}}@media(forced-colors:active){#host-media{width:30px}}@media(prefers-contrast:more){#host-media{height:40px}}@media(hover:hover){#host-media{height:50px}}";
  document.head.append(style);
  const div = document.createElement("div");
  div.id = "host-media";
  document.body.append(div);
  const box = div.getBoundingClientRect();
  return { matches, box: [box.width, box.height] };
})();
