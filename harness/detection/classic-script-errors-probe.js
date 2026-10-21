(() =>
  [window, document.querySelector("iframe").contentWindow].map((win) => ({
    continued: win.continued,
    resources: win.resources,
    errors: win.errors.map((error) => ({
      ...error,
      source: error.source ? new URL(error.source).pathname : "",
    })),
  })))();
