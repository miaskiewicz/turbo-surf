(async () => {
  const response = await fetch("/upload?id=buffered", { method: "POST", body: "x" });
  return { status: response.status, data: await response.json() };
})();
