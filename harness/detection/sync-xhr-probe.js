(() => {
  // Synchronous XMLHttpRequest on a window (turbo-surf#138): the blocking load over
  // http(s), blob: and data: URLs, the event order, the window-only responseType and
  // timeout restrictions, and network errors thrown from send().
  const out = {};
  const error = (value) => [value?.name ?? typeof value, value?.message ?? String(value)];
  const attempt = (run) => {
    try {
      return run();
    } catch (value) {
      return error(value);
    }
  };
  const blob = (parts, type) => URL.createObjectURL(new Blob(parts, { type }));
  const text = blob(["hello sync"], "text/plain");
  const masked = (value, url) =>
    Array.isArray(value) ? value.map((item) => String(item).split(url).join("<url>")) : value;
  const run = (method, url, setup = () => {}, body = null) => {
    const request = new XMLHttpRequest();
    const log = [];
    for (const name of [
      "readystatechange",
      "loadstart",
      "progress",
      "load",
      "error",
      "abort",
      "timeout",
      "loadend",
    ])
      request.addEventListener(name, (event) =>
        log.push(
          name === "readystatechange"
            ? `${name}:${request.readyState}`
            : `${name}:${request.readyState}:${event.loaded}:${event.total}:${event.lengthComputable}`,
        ),
      );
    request.upload.addEventListener("loadstart", () => log.push("upload:loadstart"));
    const thrown = attempt(() => {
      request.open(method, url, false);
      log.push("opened");
      setup(request);
      request.send(body);
      log.push("sent");
      return null;
    });
    return { request, log, thrown: masked(thrown, url) };
  };
  const summary = ({ request, log, thrown }, url) => [
    log,
    thrown,
    request.readyState,
    request.status,
    request.statusText,
    request.responseURL === url,
    request.getAllResponseHeaders(),
    request.responseText,
  ];

  out.blob = summary(run("GET", text), text);
  out.blobResponse = (() => {
    const { request } = run("GET", text);
    return [request.response, request.getResponseHeader("content-length"), request.responseType];
  })();
  out.data = summary(run("GET", "data:text/plain,abc"), "data:text/plain,abc");
  out.http = (() => {
    const { request, log, thrown } = run("GET", location.href);
    return [
      log.filter((entry) => !entry.startsWith("progress")),
      thrown,
      request.readyState,
      request.status,
      request.statusText,
      request.responseURL === location.href,
      request.getResponseHeader("content-type"),
      request.responseText.includes("<body>"),
    ];
  })();
  out.httpHeader = (() => {
    const { request, thrown } = run("GET", location.href, (value) =>
      value.setRequestHeader("X-Probe", "1"),
    );
    return [thrown, request.status];
  })();
  out.xml = (() => {
    const xml = blob(["<root><leaf/></root>"], "application/xml");
    const { request } = run("GET", xml);
    return [
      request.responseXML?.documentElement?.nodeName ?? null,
      request.responseXML?.contentType ?? null,
    ];
  })();
  out.override = (() => {
    const { request } = run("GET", text, (value) => value.overrideMimeType("text/html"));
    return [request.getResponseHeader("content-type"), request.responseXML];
  })();
  out.missing = summary(
    run("GET", `blob:${location.origin}/00000000-0000-4000-8000-000000000000`),
    `blob:${location.origin}/00000000-0000-4000-8000-000000000000`,
  );
  out.post = summary(
    run("POST", text, () => {}, "x"),
    text,
  );
  out.foreign = (() => {
    const foreign = text.replace(location.origin, "http://example.invalid");
    return summary(run("GET", foreign), foreign);
  })();
  out.sendTwice = (() => {
    const { request } = run("GET", text);
    return [request.readyState, attempt(() => request.send())];
  })();
  out.reopen = (() => {
    const { request } = run("GET", text);
    request.open("GET", "data:,second", false);
    const state = [request.readyState, request.status, request.responseText];
    request.send();
    return [...state, request.readyState, request.status, request.responseText];
  })();

  // Window-only restrictions on responseType and timeout.
  out.responseTypeBeforeOpen = (() => {
    const request = new XMLHttpRequest();
    request.responseType = "json";
    return [attempt(() => request.open("GET", text, false)), request.readyState];
  })();
  out.responseTypeAfterOpen = (() => {
    const request = new XMLHttpRequest();
    request.open("GET", text, false);
    return [
      attempt(() => {
        request.responseType = "arraybuffer";
      }),
      attempt(() => {
        request.responseType = "";
      }),
      attempt(() => {
        request.responseType = "bogus";
      }),
      request.responseType,
    ];
  })();
  out.responseTypeAsyncThenSync = (() => {
    const request = new XMLHttpRequest();
    request.open("GET", text);
    request.responseType = "text";
    return [attempt(() => request.open("GET", text, false)), request.readyState];
  })();
  out.timeoutBeforeOpen = (() => {
    const request = new XMLHttpRequest();
    request.timeout = 5;
    return [attempt(() => request.open("GET", text, false)), request.readyState, request.timeout];
  })();
  out.timeoutAfterOpen = (() => {
    const request = new XMLHttpRequest();
    request.open("GET", text, false);
    return [
      attempt(() => {
        request.timeout = 5;
      }),
      attempt(() => {
        request.timeout = 0;
      }),
      request.timeout,
    ];
  })();
  out.withCredentials = (() => {
    const { request, thrown } = run("GET", text, (value) => {
      value.withCredentials = true;
    });
    return [thrown, request.status, request.withCredentials];
  })();
  out.asyncDefault = [undefined, null, 0, "", true].map((flag) => {
    const request = new XMLHttpRequest();
    request.open("GET", "data:,x", flag);
    request.send();
    return request.readyState;
  });
  return JSON.stringify(out);
})();
