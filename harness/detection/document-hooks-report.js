// Served to cross-origin and opaque children, which report their own facts to the top page.
(() => {
  let set;
  try {
    document.domain = location.hostname;
    set = "ok";
  } catch (error) {
    set = `${error.name}: ${error.message}`;
  }
  parent.postMessage(
    {
      name: new URLSearchParams(location.search).get("name"),
      domain: document.domain,
      set,
      originAgentCluster: window.originAgentCluster,
      fullscreenEnabled: document.fullscreenEnabled,
      webkitFullscreenEnabled: document.webkitFullscreenEnabled,
      pictureInPictureEnabled: document.pictureInPictureEnabled,
    },
    "*",
  );
})();
