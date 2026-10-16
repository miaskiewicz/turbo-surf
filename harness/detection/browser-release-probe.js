(async () => ({
  userAgent: navigator.userAgent,
  brands: navigator.userAgentData.brands,
  high: await navigator.userAgentData.getHighEntropyValues(["uaFullVersion", "fullVersionList"]),
}))();
