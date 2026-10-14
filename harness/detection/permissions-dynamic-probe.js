(async () => {
  const frame = document.createElement("iframe");
  frame.setAttribute("allow", "geolocation 'none'");
  const url = new URL(location.href);
  url.searchParams.set("child", "1");
  frame.src = url.href;
  await new Promise((resolve) => {
    frame.onload = resolve;
    document.body.append(frame);
  });
  const child = frame.contentWindow;
  const status = await child.navigator.permissions.query({ name: "geolocation" });
  let changes = 0;
  status.addEventListener("change", () => changes++);
  const same = frame.featurePolicy === frame.featurePolicy;
  const rows = [];
  for (const [stage, value] of [
    ["initial", null],
    ["wildcard", "geolocation *"],
    ["self", "geolocation 'self'"],
    ["none", "geolocation 'none'"],
    ["removed", ""],
  ]) {
    if (value !== null) {
      if (value) frame.setAttribute("allow", value);
      else frame.removeAttribute("allow");
    }
    await new Promise((resolve) => setTimeout(resolve, 0));
    const normalize = (list) => list.map((value) => value.replace(location.origin, "<origin>"));
    rows.push({
      stage,
      frame: frame.featurePolicy.allowsFeature("geolocation"),
      frameList: normalize(frame.featurePolicy.getAllowlistForFeature("geolocation")),
      child: child.document.featurePolicy.allowsFeature("geolocation"),
      childList: normalize(child.document.featurePolicy.getAllowlistForFeature("geolocation")),
      oldStatus: status.state,
      newStatus: (await child.navigator.permissions.query({ name: "geolocation" })).state,
      changes,
    });
  }
  frame.remove();
  return { same, identity: frame.featurePolicy === child.document.featurePolicy, rows };
})();
