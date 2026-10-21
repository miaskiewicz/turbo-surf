// A startup socket can close without ever opening or emitting an error.
export function waitForWebSocketOpen(socket, timeoutMs = 30000) {
  return new Promise((resolve, reject) => {
    const cleanup = () => {
      clearTimeout(timer);
      socket.removeEventListener("open", opened);
      socket.removeEventListener("error", failed);
      socket.removeEventListener("close", closed);
    };
    const finish = (error) => {
      cleanup();
      if (error) reject(error);
      else resolve();
    };
    const opened = () => finish();
    const failed = () => finish(new Error("Chrome WebSocket failed before opening"));
    const closed = () => finish(new Error("Chrome WebSocket closed before opening"));
    const timer = setTimeout(() => finish(new Error("Chrome WebSocket open timed out")), timeoutMs);
    socket.addEventListener("open", opened);
    socket.addEventListener("error", failed);
    socket.addEventListener("close", closed);
    if (socket.readyState === 1) opened();
    else if (socket.readyState === 2 || socket.readyState === 3) closed();
  });
}
