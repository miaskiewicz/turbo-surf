(async () => {
  const result = [];
  for (const mode of [
    "all-metadata",
    "action",
    "method",
    "enctype",
    "target",
    "prevent-default",
    "submit-bypass",
    "invalid",
    "cancel-submit",
    "csp-blocked",
  ]) {
    const frames = [];
    for (let i = 0; i < 2; i++) {
      const frame = document.createElement("iframe");
      frame.name = `event-${mode}-${i}`;
      document.body.append(frame);
      frames.push(frame);
    }
    const form = document.createElement("form");
    form.action = `${location.origin}/wire?before=1#before`;
    form.target = frames[0].name;
    const input = document.createElement("input");
    input.name = "initial";
    input.value = mode === "invalid" ? "" : "one";
    input.required = mode === "invalid";
    input.addEventListener("invalid", () => events.push("invalid"));
    form.append(input);
    document.body.append(form);
    const events = [];
    form.addEventListener("submit", (event) => {
      events.push("submit");
      if (mode === "cancel-submit") event.preventDefault();
    });
    form.addEventListener("formdata", (event) => {
      events.push({
        type: event.type,
        trusted: event.isTrusted,
        bubbles: event.bubbles,
        cancelable: event.cancelable,
        entries: [...event.formData],
      });
      event.formData.set("initial", "changed");
      event.formData.append("event", "two");
      if (mode === "all-metadata" || mode === "action")
        form.action = `${location.origin}/wire?after=1#after`;
      if (mode === "all-metadata" || mode === "method") form.method = "post";
      if (mode === "all-metadata" || mode === "enctype") form.enctype = "text/plain";
      if (mode === "all-metadata" || mode === "target") form.target = frames[1].name;
      if (mode === "prevent-default") event.preventDefault();
    });
    const completed = new Promise((resolve) => {
      for (let i = 0; i < frames.length; i++)
        frames[i].addEventListener("load", () => {
          if (frames[i].contentWindow.location.pathname === "/wire") resolve(i);
        });
    });
    if (mode === "csp-blocked") {
      document.addEventListener("securitypolicyviolation", (event) =>
        events.push({ policy: event.effectiveDirective, disposition: event.disposition }),
      );
      const meta = document.createElement("meta");
      meta.httpEquiv = "Content-Security-Policy";
      meta.content = "form-action 'none'";
      document.head.append(meta);
    }
    if (mode === "submit-bypass") form.submit();
    else form.requestSubmit();
    if (["invalid", "cancel-submit", "csp-blocked"].includes(mode)) {
      await new Promise((resolve) => setTimeout(resolve, 100));
      result.push({ mode, events, wire: null });
      form.remove();
      for (const frame of frames) frame.remove();
      continue;
    }
    const frameIndex = await completed;
    result.push({
      mode,
      events,
      frameIndex,
      wire: JSON.parse(frames[frameIndex].contentDocument.body.textContent),
      fragment: frames[frameIndex].contentWindow.location.hash,
    });
    form.remove();
    for (const frame of frames) frame.remove();
  }
  return result;
})();
