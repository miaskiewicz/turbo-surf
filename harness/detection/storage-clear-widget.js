addEventListener("message", async (event) => {
  if (event.data.command === "clear") {
    await (await fetch("/clear?case=widgetSelf&header=%22storage%22")).text();
  }
  if (event.data.command === "seed") {
    localStorage.setItem("clear", "child");
    sessionStorage.setItem("clear", "child");
  }
  parent.postMessage(
    {
      id: event.data.id,
      state: { local: localStorage.getItem("clear"), session: sessionStorage.getItem("clear") },
    },
    "*",
  );
});
parent.postMessage({ ready: true }, "*");
