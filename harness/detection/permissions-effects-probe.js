(async () => {
  const result = [];
  for (const name of [
    "geolocation",
    "camera",
    "microphone",
    "midi",
    "clipboard-read",
    "clipboard-write",
  ]) {
    try {
      result.push([name, (await navigator.permissions.query({ name })).state]);
    } catch (error) {
      result.push([name, error.name]);
    }
  }
  return result;
})();
