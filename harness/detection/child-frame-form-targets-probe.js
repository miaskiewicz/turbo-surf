(async () => {
  // Child-frame form navigations beyond the submitting document's own frames:
  // the submitter's form-action across redirect hops (turbo-surf#139) and target
  // names resolved over the whole frame tree (turbo-surf#140). Served by the
  // control's `/echo`, `/redirect…`, `/self-csp…` and `/xo-submit` routes.
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
    form(element.contentDocument, { action, method }, fields).submit();
    await sleep(500);
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

  // turbo-surf#140: names resolve over the whole frame tree.
  out.sibling = await (async () => {
    const source = await frame(document, "/sibling-source");
    const target = await frame(document, "/sibling-target", { name: "sib" });
    const navigated = loaded(target);
    form(source.contentDocument, {
      action: "/echo/sibling",
      method: "post",
      target: "sib",
    }).submit();
    await settled(navigated);
    return [echo(target), where(source)];
  })();
  out.namedAncestor = await (async () => {
    const host = await frame(document, "/outer-host", { name: "outerhost" });
    const inner = await frame(host.contentDocument, "/outer-inner");
    const navigated = loaded(host);
    form(inner.contentDocument, {
      action: "/echo/ancestor",
      method: "post",
      target: "outerhost",
    }).submit();
    await settled(navigated);
    return echo(host);
  })();
  out.cousin = await (async () => {
    const left = await frame(document, "/left");
    const leftChild = await frame(left.contentDocument, "/left-child");
    const right = await frame(document, "/right");
    const rightChild = await frame(right.contentDocument, "/right-child", { name: "cousin" });
    const navigated = loaded(rightChild);
    form(leftChild.contentDocument, {
      action: "/echo/cousin",
      method: "post",
      target: "cousin",
    }).submit();
    await settled(navigated);
    return [echo(rightChild), where(leftChild), where(right)];
  })();
  out.grandchild = await (async () => {
    const middle = await frame(document, "/middle");
    const deep = await frame(middle.contentDocument, "/deep", { name: "grandkid" });
    const navigated = loaded(deep);
    const element = form(document, {
      action: "/echo/grandkid",
      method: "post",
      target: "grandkid",
    });
    element.submit();
    await settled(navigated);
    element.remove();
    return [echo(deep), where(middle), location.pathname];
  })();
  out.selfByName = await (async () => {
    const element = await frame(document, "/me", { name: "me" });
    const navigated = loaded(element);
    form(element.contentDocument, { action: "/echo/me", method: "post", target: "me" }).submit();
    await settled(navigated);
    return echo(element);
  })();
  // The submitter's own subtree is searched before the rest of the tree.
  out.subtreeFirst = await (async () => {
    const far = await frame(document, "/far", { name: "near" });
    const source = await frame(document, "/subtree-source");
    const close = await frame(source.contentDocument, "/close", { name: "near" });
    const farState = counted(far);
    const navigated = loaded(close);
    form(source.contentDocument, { action: "/echo/near", method: "post", target: "near" }).submit();
    await settled(navigated);
    await sleep(100);
    return [echo(close), farState.loads, where(far)];
  })();
  // Duplicate names elsewhere in the tree: the first in frame-tree order wins.
  out.duplicates = await (async () => {
    const source = await frame(document, "/dup-source");
    const first = await frame(document, "/dup-first", { name: "dup" });
    const second = document.createElement("iframe");
    second.name = "dup";
    second.src = "/dup-second";
    const ready = loaded(second);
    document.body.insertBefore(second, first);
    await ready;
    const states = [counted(first), counted(second)];
    form(source.contentDocument, { action: "/echo/dup", method: "post", target: "dup" }).submit();
    await sleep(500);
    return [states[0].loads, where(first), states[1].loads, where(second)];
  })();
  // A cross-origin submitter navigates only frames it shares its own or an
  // ancestor's origin with: the first "xpick" (this origin) is skipped for the
  // second (the submitter's). The localhost page submits from its own script.
  out.crossOrigin = await (async () => {
    const other = await frame(document, "/plain-other", { name: "xpick" });
    const shared = await frame(document, `${xo}/plain-shared`, { name: "xpick" });
    const states = [counted(other), counted(shared)];
    const targets = encodeURIComponent(JSON.stringify([["xpick", `${xo}/echo/xpick`]]));
    await frame(document, `${xo}/xo-submit?targets=${targets}`);
    await sleep(500);
    return [states[0].loads, where(other), states[1].loads];
  })();
  // javascript: actions run in the chosen navigable.
  out.javascriptTargets = await (async () => {
    const host = await frame(document, "/js-host", { name: "jshost" });
    const inner = await frame(host.contentDocument, "/js-inner");
    const sibling = await frame(document, "/js-sibling", { name: "jssibling" });
    const run = (target, name) =>
      form(inner.contentDocument, {
        action: `javascript:void(window.${name}=location.pathname)`,
        target,
      }).submit();
    run("_parent", "viaParent");
    run("_top", "viaTop");
    run("jssibling", "viaName");
    await sleep(300);
    const read = (window) => [window.viaParent, window.viaTop, window.viaName];
    return [
      read(host.contentWindow),
      read(window),
      read(sibling.contentWindow),
      read(inner.contentWindow),
    ];
  })();
  return JSON.stringify(out);
})();
