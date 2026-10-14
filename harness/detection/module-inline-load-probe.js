(async () => {
  const events = [],
    scripts = [];
  for (const [id, type] of [
    ["classic", ""],
    ["module", "module"],
  ]) {
    const script = document.createElement("script");
    if (type) script.type = type;
    script.textContent = `globalThis.__inlineLoad${id}=true`;
    script.onload = (event) =>
      events.push({
        id,
        type: event.type,
        trusted: event.isTrusted,
        target: event.target === script,
      });
    script.onerror = (event) =>
      events.push({
        id,
        type: event.type,
        trusted: event.isTrusted,
        target: event.target === script,
      });
    document.body.append(script);
    scripts.push(script);
  }
  await new Promise((resolve) => setTimeout(resolve, 200));
  const result = {
    classic: globalThis.__inlineLoadclassic === true,
    module: globalThis.__inlineLoadmodule === true,
    events,
  };
  for (const script of scripts) script.remove();
  return result;
})();
