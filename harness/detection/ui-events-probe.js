(() => {
  const interfaces = {
    UIEvent: ["view", "detail", "which", "sourceCapabilities"],
    MouseEvent: [
      "screenX",
      "screenY",
      "clientX",
      "clientY",
      "ctrlKey",
      "shiftKey",
      "altKey",
      "metaKey",
      "button",
      "buttons",
      "relatedTarget",
      "pageX",
      "pageY",
      "x",
      "y",
      "movementX",
      "movementY",
      "offsetX",
      "offsetY",
      "which",
    ],
    KeyboardEvent: [
      "key",
      "code",
      "location",
      "ctrlKey",
      "shiftKey",
      "altKey",
      "metaKey",
      "repeat",
      "isComposing",
      "charCode",
      "keyCode",
      "which",
    ],
    InputEvent: ["data", "isComposing", "inputType", "dataTransfer"],
    FocusEvent: ["relatedTarget"],
    PointerEvent: [
      "pointerId",
      "width",
      "height",
      "pressure",
      "tangentialPressure",
      "tiltX",
      "tiltY",
      "twist",
      "pointerType",
      "isPrimary",
      "altitudeAngle",
      "azimuthAngle",
      "persistentDeviceId",
    ],
    WheelEvent: ["deltaX", "deltaY", "deltaZ", "deltaMode"],
  };
  const rows = {};
  for (const [name, fields] of Object.entries(interfaces)) {
    const Constructor = globalThis[name];
    const prototype = Constructor.prototype;
    const event = new Constructor("probe", {
      key: "A",
      code: "KeyA",
      detail: 2,
      clientX: 4,
      clientY: 8,
      screenX: 12,
      screenY: 20,
      charCode: 65,
      keyCode: 65,
      shiftKey: true,
      buttons: 1,
      data: "a",
      inputType: "insertText",
      pointerId: 3,
      width: 4,
      pressure: 0.5,
      deltaY: 12,
      arbitrary: "discard",
    });
    const defaults = new Constructor("probe");
    let required = false,
      newRequired = false;
    try {
      new Constructor();
    } catch (error) {
      required = error instanceof TypeError;
    }
    try {
      Constructor("probe");
    } catch (error) {
      newRequired = error instanceof TypeError;
    }
    rows[name] = {
      constructorLength: Constructor.length,
      parent: Object.getPrototypeOf(prototype).constructor.name,
      ownKeys: Reflect.ownKeys(event).map(String).sort(),
      prototypeKeys: Reflect.ownKeys(prototype).map(String).sort(),
      tag: Object.prototype.toString.call(event),
      required,
      newRequired,
      arbitraryAbsent: !("arbitrary" in event),
      values: Object.fromEntries(fields.map((field) => [field, event[field]])),
      defaults: Object.fromEntries(fields.map((field) => [field, defaults[field]])),
      fields: Object.fromEntries(
        fields.map((field) => {
          let owner = prototype;
          while (owner && !Object.hasOwn(owner, field)) owner = Object.getPrototypeOf(owner);
          const descriptor = owner && Object.getOwnPropertyDescriptor(owner, field);
          let invalid = false;
          try {
            descriptor?.get?.call({});
          } catch (error) {
            invalid = error instanceof TypeError;
          }
          return [
            field,
            {
              owner: owner?.constructor.name,
              enumerable: descriptor?.enumerable,
              configurable: descriptor?.configurable,
              readonly: !!descriptor?.get && !descriptor.set,
              invalid,
            },
          ];
        }),
      ),
    };
  }
  return JSON.stringify(rows);
})();
