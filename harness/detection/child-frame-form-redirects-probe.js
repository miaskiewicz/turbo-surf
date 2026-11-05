(async () => {
  // Child-frame form navigations across redirects: the submitter's form-action
  // governs every hop (turbo-surf#139). Served by the control's `/echo`,
  // `/redirect…` and `/self-csp…` routes.
  const out = {};
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  // A navigation that never commits reports itself instead of stalling the probe.
  const settled = (navigation) => Promise.race([navigation, sleep(3000)]);
  const loaded = (frame) =>
    new Promise((resolve) => frame.addEventListener("load", resolve, { once: true }));
  const frame = async (parent, src, attributes = {}) => {
    const element = parent.createElement("iframe");
    for (const [name, value] of Object.entries(attributes)) element.setAttribute(name, value);
    element.src = src;
    const ready = loaded(element);
    parent.body.append(element);
    await ready;
    return element;
  };
  const echo = (element) => {
    const text = element.contentDocument?.getElementById("echo")?.textContent;
    return text ? JSON.parse(text) : ["no echo", where(element)];
  };
  const where = (element) => {
    try {
      return element.contentWindow.location.pathname;
    } catch (error) {
      return error.name;
    }
  };
  const form = (document, attributes, fields = [["f", "1"]]) => {
    const element = document.createElement("form");
    for (const [name, value] of Object.entries(attributes)) element.setAttribute(name, value);
    for (const [name, value] of fields) {
      const input = document.createElement("input");
      input.name = name;
      input.value = value;
      element.append(input);
    }
    document.body.append(element);
    return element;
  };
  const counted = (element) => {
    const state = { loads: 0 };
    element.addEventListener("load", () => state.loads++);
    return state;
  };
  const xo = `http://localhost:${location.port}`;
  const redirect = (status, to) => `/redirect${status}?to=${encodeURIComponent(to)}`;

  // turbo-surf#139: the submitter's form-action governs every redirect hop.
  const redirected = async (src, action, method = "post", fields = undefined) => {
    const element = await frame(document, src);
    const violations = [];
    element.contentDocument.addEventListener("securitypolicyviolation", (event) =>
      violations.push([
        event.violatedDirective,
        event.blockedURI
          .replace(location.origin, "<origin>")
          .replace(xo, "<xo>")
          .replace(encodeURIComponent(xo), "<xo>"),
      ]),
    );
    const state = counted(element);
    const navigated = loaded(element);
    form(element.contentDocument, { action, method }, fields).submit();
    await settled(navigated);
    await sleep(100);
    return [state.loads, where(element), violations, echo(element)];
  };
  out.redirectCrossOriginBlocked = await redirected(
    "/self-csp-a",
    redirect(302, `${xo}/echo/blocked`),
  );
  out.redirectSameOriginAllowed = await redirected("/self-csp-b", redirect(302, "/echo/allowed"));
  out.redirect307Allowed = await redirected("/self-csp-c", redirect(307, "/echo/kept"));
  // A GET submission replaces the action's query, so the hop comes from a field.
  out.redirectGetBlocked = await redirected("/self-csp-d", "/redirect302", "get", [
    ["to", `${xo}/echo/blocked-get`],
  ]);
  // A document targeting its own named child: its policy, not the child's, applies.
  out.redirectNamedChildBlocked = await (async () => {
    const host = await frame(document, "/self-csp-host");
    const kid = await frame(host.contentDocument, "/kid", { name: "policykid" });
    const state = counted(kid);
    form(host.contentDocument, {
      action: redirect(302, `${xo}/echo/kid`),
      method: "post",
      target: "policykid",
    }).submit();
    await sleep(500);
    return [state.loads, where(kid), echo(kid)];
  })();

  return JSON.stringify(out);
})();
