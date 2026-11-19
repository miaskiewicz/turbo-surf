(async () =>
  new Promise((resolve) => {
    const frame = document.createElement("iframe");
    document.body.append(frame);
    const child = frame.contentWindow;
    const order = [];
    let message;
    const finish = () => {
      if (order.length === 2) resolve({ order, message });
    };
    child.addEventListener(
      "message",
      (event) => {
        message = {
          data: event.data,
          sourceIsWindow: event.source === window,
          currentTargetIsChild: event.currentTarget === child,
          originMatches: event.origin === location.origin,
          trusted: event.isTrusted,
        };
        order.push("message");
        finish();
      },
      { once: true },
    );
    setTimeout(() => {
      const started = Date.now();
      while (Date.now() - started < 60) {}
      child.postMessage("task-clock-message", "*");
      setTimeout(() => {
        order.push("timer");
        finish();
      }, 40);
    }, 0);
  }))();
