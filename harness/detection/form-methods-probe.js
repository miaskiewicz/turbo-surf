(() => {
  const result = {};
  const describe = (error) => (error ? error.name : null);
  const attempt = (callback) => {
    try {
      const value = callback();
      return { value: value === undefined ? "undefined" : value, error: null };
    } catch (error) {
      return { error: describe(error) };
    }
  };
  function fixture(content = '<input id="field"><button id="send">Send</button>') {
    const host = document.createElement("div");
    host.innerHTML = "<form>" + content + "</form>";
    document.body.append(host);
    return {
      host,
      form: host.querySelector("form"),
      input: host.querySelector("input"),
      button: host.querySelector("button"),
    };
  }
  const descriptorRow = (owner, method) => {
    const descriptor = Object.getOwnPropertyDescriptor(globalThis[owner].prototype, method);
    return descriptor
      ? {
          length: descriptor.value.length,
          name: descriptor.value.name,
          enumerable: descriptor.enumerable,
          writable: descriptor.writable,
          configurable: descriptor.configurable,
        }
      : null;
  };
  const controlRow = (tag) => {
    const control = document.createElement(tag);
    return [
      control.willValidate,
      control.validationMessage,
      control.validity?.valid,
      control.checkValidity(),
      control.reportValidity(),
    ];
  };
  for (const [owner, method] of [
    ["HTMLFormElement", "requestSubmit"],
    ["HTMLFormElement", "submit"],
    ["HTMLFormElement", "checkValidity"],
    ["HTMLFormElement", "reportValidity"],
    ["HTMLInputElement", "setCustomValidity"],
  ])
    result[owner + "." + method] = descriptorRow(owner, method);
  for (const tag of ["input", "textarea", "select", "button", "fieldset", "object", "output"])
    result[tag] = controlRow(tag);
  {
    const { form, input, host } = fixture("<input required><button>Send</button>");
    const events = [];
    input.addEventListener("invalid", (event) => {
      events.push([
        event.type,
        event.isTrusted,
        event.bubbles,
        event.cancelable,
        event.target === input,
      ]);
      event.preventDefault();
    });
    form.addEventListener("invalid", () => events.push("bubbled"));
    result.required = [
      input.willValidate,
      input.validity.valueMissing,
      input.validity.valid,
      input.checkValidity(),
      form.checkValidity(),
      events,
    ];
    input.value = "filled";
    result.filled = [input.validity.valueMissing, input.validity.valid, form.checkValidity()];
    input.setCustomValidity("own message");
    result.custom = [
      input.validationMessage,
      input.validity.customError,
      input.validity.valid,
      input.checkValidity(),
    ];
    input.setCustomValidity("");
    result.cleared = [input.validationMessage, input.validity.customError, input.checkValidity()];
    result.customSymbol = attempt(() => input.setCustomValidity(Symbol("x")));
    result.customMissing = attempt(() => input.setCustomValidity());
    host.remove();
  }
  for (const attribute of ["disabled", "readonly", 'type="hidden"']) {
    const { input, form, host } = fixture(
      "<input required " + attribute + "><button>Send</button>",
    );
    result[attribute] = [
      input.willValidate,
      input.validity.valueMissing,
      input.validity.valid,
      input.checkValidity(),
      form.checkValidity(),
    ];
    host.remove();
  }
  const requestFixture = (kind) =>
    fixture(
      kind === "input"
        ? '<input type="submit" id="send">'
        : '<input required><button id="send">Send</button>',
    );
  const listenRequest = (f, log) => {
    f.form.addEventListener("submit", (event) => {
      log.push({
        type: event.type,
        constructor: event.constructor.name,
        trusted: event.isTrusted,
        bubbles: event.bubbles,
        cancelable: event.cancelable,
        composed: event.composed,
        submitter: event.submitter?.id || null,
      });
      event.preventDefault();
    });
    if (f.input)
      f.input.addEventListener("invalid", (event) => {
        log.push({ type: "invalid", trusted: event.isTrusted });
        event.preventDefault();
      });
  };
  const configureRequest = (kind, f) => {
    if (f.input && kind !== "required" && kind !== "novalidate" && kind !== "formnovalidate")
      f.input.value = "ok";
    if (kind === "novalidate") f.form.noValidate = true;
    if (kind === "formnovalidate") f.button.formNoValidate = true;
    let foreign;
    if (kind === "foreign") {
      foreign = fixture();
      foreign.button.id = "foreign";
    }
    if (kind === "non-submit") f.button.type = "button";
    return foreign;
  };
  const omittedSubmit = (f) => f.form.requestSubmit();
  const requestSubmits = {
    omitted: omittedSubmit,
    required: omittedSubmit,
    novalidate: omittedSubmit,
    undefined: (f) => f.form.requestSubmit(undefined),
    null: (f) => f.form.requestSubmit(null),
    input: (f) => f.form.requestSubmit(f.input),
    foreign: (f, foreign) => f.form.requestSubmit(foreign.button),
  };
  const buttonSubmit = (f) => f.form.requestSubmit(f.button);
  const requestRow = (kind) => {
    const f = requestFixture(kind);
    const log = [];
    listenRequest(f, log);
    const foreign = configureRequest(kind, f);
    const submit = requestSubmits[kind] ?? buttonSubmit;
    const row = [attempt(() => submit(f, foreign)), log];
    f.host.remove();
    foreign?.host.remove();
    return row;
  };
  for (const kind of [
    "omitted",
    "undefined",
    "null",
    "button",
    "input",
    "foreign",
    "non-submit",
    "required",
    "novalidate",
    "formnovalidate",
  ])
    result["request-" + kind] = requestRow(kind);
  const validityKeys = [
    "valueMissing",
    "typeMismatch",
    "patternMismatch",
    "tooLong",
    "tooShort",
    "rangeUnderflow",
    "rangeOverflow",
    "stepMismatch",
    "badInput",
    "customError",
    "valid",
  ];
  for (const [id, markup, value] of [
    ["email-missing-at", '<input type="email">', "bad"],
    ["email-empty-user", '<input type="email">', "@x.test"],
    ["email-empty-host", '<input type="email">', "x@"],
    ["email-invalid-host", '<input type="email">', "x@bad..host"],
    ["email-multiple", '<input type="email" multiple>', "first@a.test, second@b.test"],
    ["url-valid", '<input type="url">', "https://example.test/a"],
    ["url-invalid", '<input type="url">', "relative/path"],
    ["pattern", '<input pattern="[A-Z]{2}">', "ab"],
    ["pattern-invalid", '<input pattern="[">', "abc"],
    ["number-min", '<input type="number" min="5">', "2"],
    ["number-max", '<input type="number" max="5">', "8"],
    ["number-step", '<input type="number" step="2">', "3"],
    ["checkbox-required", '<input type="checkbox" required>', ""],
    ["textarea-required", "<textarea required></textarea>", ""],
    [
      "select-required",
      '<select required><option value="">Choose</option><option value="a">A</option></select>',
      "",
    ],
  ]) {
    const host = document.createElement("div");
    host.innerHTML = markup;
    document.body.append(host);
    const control = host.firstElementChild;
    control.value = value;
    const flags = {};
    for (const key of validityKeys) flags[key] = control.validity[key];
    result["flags-" + id] = [flags, control.validationMessage];
    host.remove();
  }
  return result;
})();
