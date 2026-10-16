(() => {
  globalThis.unreadControl = {
    async check(url) {
      const controller = new AbortController();
      const timeout = setTimeout(() => controller.abort(), 750);
      try {
        const response = await fetch(url, {
          cache: "only-if-cached",
          mode: "same-origin",
          signal: controller.signal,
        });
        const body = await response.text();
        return { status: response.status, length: body.length };
      } catch (error) {
        return { error: error.name };
      } finally {
        clearTimeout(timeout);
      }
    },
    retire() {
      return new Promise((resolve) => {
        const frame = document.createElement("iframe");
        const listener = (event) => {
          if (event.data?.kind === "unread-frame") {
            window.removeEventListener("message", listener);
            frame.remove();
            resolve();
          }
        };
        window.addEventListener("message", listener);
        frame.src = "/unread-frame";
        document.body.append(frame);
      });
    },
  };
  return true;
})();
