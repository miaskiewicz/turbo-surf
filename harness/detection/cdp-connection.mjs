// Flattened CDP sessions share a socket, but commands and events retain their target.
// The per-command timeout is a generous cap, not a timing: a loaded host can keep a
// healthy Chrome from answering for seconds (turbo-surf#50).
export function cdpConnection(socket, onEvent, timeoutMs = 60000) {
  let sequence = 0;
  const pending = new Map();
  socket.onmessage = ({ data }) => {
    const message = JSON.parse(data);
    if (!message.id) {
      onEvent(message);
      return;
    }
    const waiting = pending.get(message.id);
    if (!waiting) return;
    pending.delete(message.id);
    clearTimeout(waiting.timer);
    if (message.error) waiting.reject(new Error(message.error.message));
    else waiting.resolve(message.result || {});
  };
  return {
    call(method, params = {}, sessionId) {
      return new Promise((resolve, reject) => {
        const id = ++sequence;
        const timer = setTimeout(() => {
          pending.delete(id);
          reject(new Error(`CDP ${method} timed out`));
        }, timeoutMs);
        pending.set(id, { resolve, reject, timer });
        try {
          socket.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }));
        } catch (error) {
          pending.delete(id);
          clearTimeout(timer);
          reject(error);
        }
      });
    },
    close() {
      for (const waiting of pending.values()) {
        clearTimeout(waiting.timer);
        waiting.reject(new Error("Chrome oracle closed"));
      }
      pending.clear();
    },
  };
}
