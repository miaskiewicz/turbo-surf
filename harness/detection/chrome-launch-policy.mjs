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

// Caller-supplied browser switches (e.g. `--use-mock-keychain`), appended verbatim to both
// collectors' launch. Only `--flag` or `--flag=value` strings are accepted. A config
// without `chromeArgs` (direct collector/control runs) falls back to TURBO_SURF_CHROME_ARGS.
const envChromeArgs = (env) => (env.TURBO_SURF_CHROME_ARGS || "").split(/\s+/).filter(Boolean);
export function chromeExtraArgs(config, env = process.env) {
  const args = config.chromeArgs ?? envChromeArgs(env);
  if (!Array.isArray(args)) throw new TypeError("chromeArgs must be an array of --flags");
  for (const arg of args)
    if (typeof arg !== "string" || !/^--[A-Za-z0-9]/.test(arg))
      throw new TypeError(`chromeArgs entries must be --flags: ${JSON.stringify(arg)}`);
  return [...args];
}

// Repeatable `--chrome-arg FLAG` / `--chrome-arg=FLAG`, after whitespace-separated
// TURBO_SURF_CHROME_ARGS.
export function parseChromeArgs(argv, env = {}) {
  const args = envChromeArgs(env);
  for (const [index, argument] of argv.entries()) {
    if (argument.startsWith("--chrome-arg=")) args.push(argument.slice("--chrome-arg=".length));
    else if (argument === "--chrome-arg") {
      const value = argv[index + 1];
      if (!value) throw new Error("--chrome-arg requires a Chrome --flag");
      args.push(value);
    }
  }
  return chromeExtraArgs({ chromeArgs: args }, env);
}
