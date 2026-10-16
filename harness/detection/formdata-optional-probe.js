(() => {
  const result = [];
  for (const mode of [
    "file-undefined",
    "text-undefined",
    "file-null",
    "file-bigint",
    "modified-bigint",
  ]) {
    try {
      if (mode === "modified-bigint") {
        result.push({ mode, value: new File([], "x", { lastModified: 1n }).lastModified });
        continue;
      }
      const file = new File(["x"], "original", { lastModified: 42 }),
        data = new FormData();
      if (mode === "text-undefined") data.append("key", "text", undefined);
      else
        data.append(
          "key",
          file,
          mode === "file-undefined" ? undefined : mode === "file-null" ? null : 1n,
        );
      const value = data.get("key");
      result.push({ mode, name: value.name, same: value === file });
    } catch (error) {
      result.push({ mode, error: error.name });
    }
  }
  return result;
})();
