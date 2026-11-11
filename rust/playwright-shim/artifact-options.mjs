// Reject recording requests before allocating a context or starting runner work.
export function validateArtifactOptions(options = {}) {
  if (options.recordVideo != null) {
    throw new Error(
      "turbo-surf: recordVideo unavailable — no-browser engine has no video recorder",
    );
  }
  for (const name of ["trace", "video"]) {
    const value = options[name];
    const mode = typeof value === "object" && value !== null ? value.mode : value;
    if (value != null && mode !== "off") {
      throw new Error(
        `turbo-surf: ${name} unavailable — no-browser engine has no Playwright ${name} recorder; set use.${name} to "off"`,
      );
    }
  }
}
