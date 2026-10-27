// Optional isolation for authored localhost controls; normal browser flags stay unchanged.
export function chromeExtensionArgs(config) {
  const flags = [];
  for (const [option, values] of [
    [
      "disableExtensions",
      ["--disable-extensions", "--disable-component-extensions-with-background-pages"],
    ],
    // Frame-timing controls must not be throttled when the headed window is occluded.
    [
      "foregroundFrames",
      [
        "--disable-backgrounding-occluded-windows",
        "--disable-renderer-backgrounding",
        "--disable-background-timer-throttling",
      ],
    ],
    // Keyboard-driven controls: a headed window takes the OS keyboard focus on launch,
    // so keys typed on the host land in the focused field next to CDP's (turbo-surf#51).
    ["headless", ["--headless=new"]],
  ]) {
    if (config[option] !== true) continue;
    const host = new URL(config.url).hostname;
    if (!["localhost", "127.0.0.1", "[::1]"].includes(host))
      throw new TypeError(`${option} is restricted to authored localhost controls`);
    flags.push(...values);
  }
  return flags;
}
