import { pathToFileURL } from "node:url";
import { __attemptWorker } from "./index.mjs";
await import(pathToFileURL(process.argv[2]).href);
process.send({ kind: "ready" });
process.once("disconnect", () => process.exit(1));
let _chain = Promise.resolve();
process.on("message", (message) => {
  _chain = _chain.then(async () => {
    if (message.kind === "close") {
      let error;
      try {
        await __attemptWorker.close();
      } catch (value) {
        error = serializeError(value);
      }
      process.send({ kind: "closed", error }, () => process.exit(error ? 1 : 0));
      return;
    }
    if (message.kind !== "run") return;
    process.send({ kind: "started", startedAt: Date.now() });
    try {
      const result = await __attemptWorker.run(message);
      process.send({ kind: "result", ...result, finishedAt: Date.now() });
    } catch (error) {
      process.send({ kind: "result", error: serializeError(error), finishedAt: Date.now() });
    }
  });
});

function serializeError(error) {
  const errors = error instanceof AggregateError ? error.errors.map(serializeError) : undefined;
  return {
    message: error.message,
    stack: error.stack + (errors?.map((nested) => `\nCaused by: ${nested.stack}`).join("") ?? ""),
    errors,
  };
}
