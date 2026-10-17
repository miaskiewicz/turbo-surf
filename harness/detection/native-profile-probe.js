async (operation) => {
  if (operation.local !== undefined) localStorage.setItem("profile", operation.local);
  if (operation.session !== undefined) sessionStorage.setItem("profile", operation.session);
  if (operation.cache) await (await fetch("/cached")).text();
  if (operation.clear) await (await fetch("/clear")).text();
  return {
    local: localStorage.getItem("profile"),
    session: sessionStorage.getItem("profile"),
  };
};
