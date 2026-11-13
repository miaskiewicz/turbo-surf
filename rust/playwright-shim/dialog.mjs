export function dialogMessage(page, entry, respond) {
  let handled = false;
  const answer = async (accepted, promptText) => {
    if (handled) throw new Error("Dialog is already handled");
    if (promptText !== undefined && typeof promptText !== "string")
      throw new TypeError("promptText must be a string");
    if (accepted && entry.type === "prompt" && promptText === undefined) promptText = "";
    handled = true;
    await respond({ id: entry.id, accepted, promptText });
  };
  return {
    type: () => entry.type,
    message: () => entry.message ?? fromUnits(entry.messageUnits),
    defaultValue: () => entry.defaultValue ?? fromUnits(entry.defaultValueUnits),
    page: () => page,
    accept: (promptText) => answer(true, promptText),
    dismiss: () => answer(false),
  };
}

function fromUnits(units) {
  let result = "";
  for (let i = 0; i < units.length; i += 8192)
    result += String.fromCharCode(...units.slice(i, i + 8192));
  return result;
}

export function dialogReplyUnits(text) {
  return text === undefined
    ? undefined
    : Array.from({ length: text.length }, (_, i) => text.charCodeAt(i));
}
