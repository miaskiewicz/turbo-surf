/** Canonical effect presentation; page timers and script execution stay enabled. */
export function animationPolicy(value = "enabled") {
  if (value !== "enabled" && value !== "disabled")
    throw new TypeError("animations must be enabled or disabled");
  return value;
}
