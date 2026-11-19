const PERMISSIONS = new Set([
  "geolocation",
  "midi",
  "midi-sysex",
  "notifications",
  "camera",
  "microphone",
  "background-sync",
  "ambient-light-sensor",
  "accelerometer",
  "gyroscope",
  "magnetometer",
  "accessibility-events",
  "clipboard-read",
  "clipboard-write",
  "payment-handler",
  "persistent-storage",
  "idle-detection",
  "screen-wake-lock",
  "local-fonts",
  "storage-access",
  "top-level-storage-access",
  "window-management",
]);

export function contextGeolocation(value) {
  if (value == null) return null;
  for (const [name, limit] of [
    ["latitude", 90],
    ["longitude", 180],
  ]) {
    if (!Number.isFinite(value[name]) || Math.abs(value[name]) > limit)
      throw new Error(`Invalid geolocation ${name}`);
  }
  const accuracy = value.accuracy ?? 0;
  if (!Number.isFinite(accuracy) || accuracy < 0) throw new Error("Invalid geolocation accuracy");
  return { latitude: value.latitude, longitude: value.longitude, accuracy };
}

export function contextPermissionGrant(permissions, options = {}) {
  if (!Array.isArray(permissions) || permissions.some((name) => !PERMISSIONS.has(name)))
    throw new Error("Unsupported context permission");
  const origin = options.origin == null ? null : new URL(options.origin).origin;
  if (origin === "null") throw new Error("Invalid permission origin");
  return { origin, permissions: [...new Set(permissions)] };
}

export function mergePermissionGrant(grants, grant) {
  const previous = grants.find((entry) => entry.origin === grant.origin);
  const merged = {
    origin: grant.origin,
    permissions: [...new Set([...(previous?.permissions ?? []), ...grant.permissions])],
  };
  return [...grants.filter((entry) => entry.origin !== grant.origin), merged];
}
