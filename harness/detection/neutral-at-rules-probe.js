(() => {
  const registration =
    '@property --paint {syntax:"<color>";initial-value:transparent;inherits:false}';
  const cases = [
    [
      "absent-named-container",
      "@container probe-container (max-width:200px){#neutral-target{width:70px}}",
      "",
    ],
    [
      "eligible-container",
      "@container probe-container (max-width:200px){#neutral-target{width:70px}}",
      "container-name:probe-container;container-type:inline-size;width:100px",
    ],
    [
      "inline-container-shorthand",
      "@container probe-container (max-width:200px){#neutral-target{width:70px}}",
      "container:probe-container / inline-size;width:100px",
    ],
    [
      "paint-registration",
      registration + "#neutral-target{background:var(--paint);--paint:red;transition:--paint 50ms}",
      "",
    ],
    [
      "transitive-paint-registration",
      registration +
        "#neutral-target{--alias:var(--paint);background-image:linear-gradient(var(--alias),var(--paint));--paint:red}",
      "",
    ],
    ["direct-width-registration", registration + "#neutral-target{width:var(--paint,20px)}", ""],
    [
      "transitive-width-registration",
      registration + "#neutral-target{--alias:var(--paint);width:var(--alias,20px)}",
      "",
    ],
    [
      "border-shorthand-registration",
      registration + "#neutral-target{border:var(--paint,10px solid red)}",
      "",
    ],
    [
      "important-padding-registration",
      registration + "#neutral-target{padding:var(--paint,10px)!important}",
      "",
    ],
    ["inline-width-registration", registration, ""],
    [
      "escaped-width-registration",
      '@property --p\\61 int{syntax:"<color>";initial-value:transparent;inherits:false}#neutral-target{width:var(--p\\61 int,20px)}',
      "",
    ],
    ["view-transition-auto", "@view-transition{navigation:auto}", ""],
    ["view-transition-none", "@view-transition{navigation:none}", ""],
    ["unknown-layer", "@layer test{#neutral-target{width:70px}}", ""],
  ];
  const result = [];
  for (const [name, css, containerStyle] of cases) {
    const style = document.createElement("style"),
      parent = document.createElement("div"),
      target = document.createElement("div");
    style.textContent = "#neutral-target{box-sizing:content-box;width:10px;height:10px}" + css;
    parent.setAttribute("style", containerStyle);
    target.id = "neutral-target";
    if (name === "inline-width-registration")
      target.setAttribute("style", "width:var(--paint,20px)");
    parent.append(target);
    document.head.append(style);
    document.body.append(parent);
    try {
      const box = target.getBoundingClientRect();
      result.push({
        name,
        types: [...style.sheet.cssRules].map((rule) => rule.type),
        box: [box.width, box.height],
      });
    } catch (error) {
      result.push({ name, error: error.name, message: error.message });
    } finally {
      style.remove();
      parent.remove();
    }
  }
  return result;
})();
