const modifierCodes = {
  Shift: "ShiftLeft",
  Control: "ControlLeft",
  Alt: "AltLeft",
  Meta: "MetaLeft",
};
const aliases = {
  Space: " ",
  Esc: "Escape",
  ControlOrMeta: process.platform === "darwin" ? "Meta" : "Control",
};
const punctuation = {
  " ": "Space",
  "-": "Minus",
  "=": "Equal",
  "[": "BracketLeft",
  "]": "BracketRight",
  ";": "Semicolon",
  "'": "Quote",
  ",": "Comma",
  ".": "Period",
  "/": "Slash",
  "\\": "Backslash",
  "`": "Backquote",
};

function keyDefinition(input, pressed) {
  const name = aliases[input] ?? input;
  let key = name;
  let code = modifierCodes[name] ?? punctuation[name] ?? name;
  if (/^[a-z]$/i.test(name)) code = `Key${name.toUpperCase()}`;
  if (/^[0-9]$/.test(name)) code = `Digit${name}`;
  if (/^Key[A-Z]$/.test(name)) key = name.slice(3).toLowerCase();
  if (/^Digit[0-9]$/.test(name)) key = name.slice(5);
  if (/^(Shift|Control|Alt|Meta)(Left|Right)$/.test(name)) key = name.replace(/(Left|Right)$/, "");
  if (/^[a-z]$/.test(key) && (pressed.has("ShiftLeft") || pressed.has("ShiftRight")))
    key = key.toUpperCase();
  return { key, code, location: code.endsWith("Right") ? 2 : code.endsWith("Left") ? 1 : 0 };
}

export function makeKeyboard(page) {
  const pressed = new Set();
  const dispatch = async (type, input) => {
    await page._ensureLive();
    const definition = keyDefinition(String(input), pressed);
    const repeat = type === "keydown" && pressed.has(definition.code);
    if (type === "keydown") pressed.add(definition.code);
    else pressed.delete(definition.code);
    const has = (name) => pressed.has(`${name}Left`) || pressed.has(`${name}Right`);
    const options = {
      ...definition,
      repeat,
      shiftKey: has("Shift"),
      ctrlKey: has("Control"),
      altKey: has("Alt"),
      metaKey: has("Meta"),
    };
    await page._sessionEval(`(() => {
      const element = document.activeElement || document.body || document.documentElement;
      const options = {...${JSON.stringify(options)}, bubbles: true, cancelable: true, view: globalThis};
      element.dispatchEvent(new KeyboardEvent(${JSON.stringify(type)}, options));
      if (${JSON.stringify(type)} === 'keydown' && options.key.length === 1)
        element.dispatchEvent(new KeyboardEvent('keypress', options));
    })()`);
  };
  const down = (key) => dispatch("keydown", key);
  const up = (key) => dispatch("keyup", key);
  const press = async (chord) => {
    const parts = String(chord).split("+");
    const key = parts.pop();
    const introduced = [];
    for (const modifier of parts) {
      const definition = keyDefinition(modifier, pressed);
      if (!pressed.has(definition.code)) {
        await down(modifier);
        introduced.push(modifier);
      }
    }
    try {
      await down(key);
      await up(key);
    } finally {
      for (const modifier of introduced.reverse()) await up(modifier);
    }
  };
  return {
    press,
    down,
    up,
    async type(text) {
      for (const character of String(text)) await press(character);
    },
    insertText: (text) => press(String(text)),
  };
}
