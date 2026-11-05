(async () => {
  // Native form navigation submitted inside child frames: GET/POST encodings,
  // the navigation request headers the server sees (served by the control's
  // `/echo` route), target routing (_self, _parent, named frames), the sandbox
  // allow-forms flag and the child document's form-action policy.
  const out = {};
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
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
    return text ? JSON.parse(text) : ["no echo", element.contentWindow.location.pathname];
  };
  const form = (document, attributes, fields) => {
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
  const submitted = async (
    element,
    attributes,
    fields = [
      ["a", "1"],
      ["b", "x y&z"],
    ],
  ) => {
    const navigated = loaded(element);
    form(element.contentDocument, attributes, fields).submit();
    await navigated;
    return echo(element);
  };

  const child = await frame(document, "/child");
  out.get = await submitted(child, { action: "/echo/get", method: "get" });
  out.post = await submitted(child, { action: "/echo/post", method: "post" });
  out.text = await submitted(child, {
    action: "/echo/text",
    method: "post",
    enctype: "text/plain",
  });
  out.multipart = await submitted(child, {
    action: "/echo/multipart",
    method: "post",
    enctype: "multipart/form-data",
  });
  out.self = await submitted(child, { action: "/echo/self", method: "post", target: "_self" });
  out.defaultAction = await (async () => {
    const element = await frame(document, "/echo/start?x=1");
    return submitted(element, { method: "post" });
  })();
  out.requestSubmit = await (async () => {
    const navigated = loaded(child);
    const element = form(child.contentDocument, { action: "/echo/form", method: "get" }, [
      ["c", "3"],
    ]);
    const button = child.contentDocument.createElement("button");
    button.name = "go";
    button.value = "yes";
    button.setAttribute("formaction", "/echo/submitter");
    button.setAttribute("formmethod", "post");
    element.append(button);
    element.requestSubmit(button);
    await navigated;
    return echo(child);
  })();

  // Named descendant and _parent targets from inside the child.
  const host = await frame(document, "/host");
  const inner = await frame(host.contentDocument, "/inner", { name: "inner" });
  out.namedDescendant = await (async () => {
    const navigated = loaded(inner);
    form(host.contentDocument, { action: "/echo/named", method: "post", target: "inner" }, [
      ["n", "1"],
    ]).submit();
    await navigated;
    return [echo(inner), host.contentWindow.location.pathname];
  })();
  out.parentTarget = await (async () => {
    const navigated = loaded(host);
    form(inner.contentDocument, { action: "/echo/parent", method: "post", target: "_parent" }, [
      ["p", "1"],
    ]).submit();
    await navigated;
    return echo(host);
  })();
  // A top-level form targeting a named child frame.
  out.topToNamedChild = await (async () => {
    const named = await frame(document, "/named", { name: "kid" });
    const navigated = loaded(named);
    const element = form(document, { action: "/echo/kid", method: "post", target: "kid" }, [
      ["k", "1"],
    ]);
    element.submit();
    await navigated;
    element.remove();
    return [echo(named), location.pathname];
  })();

  // Sandboxing: forms need allow-forms; with it the child navigates as usual.
  out.sandboxBlocked = await (async () => {
    const element = await frame(document, "/sandboxed", {
      sandbox: "allow-same-origin allow-scripts",
    });
    let navigations = 0;
    element.addEventListener("load", () => navigations++);
    form(element.contentDocument, { action: "/echo/sandboxed", method: "post" }, [
      ["s", "1"],
    ]).submit();
    await sleep(300);
    return [navigations, element.contentWindow.location.pathname];
  })();
  out.sandboxAllowed = await (async () => {
    const element = await frame(document, "/sandboxed-forms", {
      sandbox: "allow-same-origin allow-forms",
    });
    return submitted(element, { action: "/echo/allowed", method: "post" }, [["s", "2"]]);
  })();
  // The child document's own form-action policy governs its submissions.
  // A sandboxed frame (even with allow-forms) cannot navigate its parent.
  out.sandboxParent = await (async () => {
    const outer = await frame(document, "/outer");
    const nested = await frame(outer.contentDocument, "/nested", {
      sandbox: "allow-same-origin allow-forms",
    });
    let navigations = 0;
    outer.addEventListener("load", () => navigations++);
    form(nested.contentDocument, { action: "/echo/escape", method: "post", target: "_parent" }, [
      ["e", "1"],
    ]).submit();
    await sleep(300);
    return [
      navigations,
      outer.contentWindow.location.pathname,
      nested.contentWindow.location.pathname,
    ];
  })();
  out.formAction = await (async () => {
    const element = await frame(document, "/csp-child");
    const violations = [];
    element.contentDocument.addEventListener("securitypolicyviolation", (event) =>
      violations.push([
        event.violatedDirective,
        event.blockedURI.replace(location.origin, "<origin>"),
      ]),
    );
    let navigations = 0;
    element.addEventListener("load", () => navigations++);
    form(element.contentDocument, { action: "/echo/csp", method: "post" }, [["c", "1"]]).submit();
    await sleep(300);
    return [navigations, element.contentWindow.location.pathname, violations];
  })();
  return JSON.stringify(out);
})();
