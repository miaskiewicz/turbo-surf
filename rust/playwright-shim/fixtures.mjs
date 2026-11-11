// Fixture graphs belong to one test attempt, including its hooks and cleanup.
export function fixtureNames(fn) {
  const match = /^[^(]*\(\s*\{([^}]*)\}/.exec(String(fn));
  if (!match) return [];
  return match[1]
    .split(",")
    .map((field) => field.trim().split(/[:=]/, 1)[0].trim())
    .filter(Boolean);
}

async function startFixture(name, def, arg, testInfo, teardowns) {
  if (typeof def !== "function") {
    arg[name] = def;
    return;
  }
  let supplied = false;
  await new Promise((settle, reject) => {
    let completion;
    const used = (value) =>
      new Promise((release) => {
        if (supplied) {
          reject(new Error(`Fixture ${name} called use more than once`));
          return;
        }
        supplied = true;
        arg[name] = value;
        teardowns.push({
          release,
          get completion() {
            return completion;
          },
        });
        settle();
      });
    completion = Promise.resolve().then(() => def(arg, used, testInfo));
    completion.then(() => {
      if (!supplied) reject(new Error(`Fixture ${name} did not call use`));
    }, reject);
  });
}

export async function openFixtures(baseDefs, extDefs, testInfo) {
  const arg = {};
  const teardowns = [];
  const resolved = new Set();
  const resolving = new Set();
  const resolveFixture = async (name, baseOnly = false) => {
    const key = `${baseOnly ? "base:" : ""}${name}`;
    if (resolved.has(key)) return;
    if (resolving.has(key)) throw new Error(`Fixture dependency cycle at ${name}`);
    const overridden = !baseOnly && Object.hasOwn(extDefs, name);
    const defs = overridden ? extDefs : baseDefs;
    if (!Object.hasOwn(defs, name)) throw new Error(`Unknown fixture ${name}`);
    resolving.add(key);
    try {
      const def = defs[name];
      for (const dependency of fixtureNames(def)) {
        await resolveFixture(dependency, overridden && dependency === name);
      }
      await startFixture(name, def, arg, testInfo, teardowns);
      resolved.add(key);
    } finally {
      resolving.delete(key);
    }
  };
  const resolve = async (names) => {
    for (const name of names) await resolveFixture(name);
  };
  const teardown = async () => {
    const errors = [];
    for (const record of teardowns.splice(0).reverse()) {
      record.release();
      try {
        await record.completion;
      } catch (error) {
        errors.push(error);
      }
    }
    if (errors.length) throw new AggregateError(errors, "Fixture teardown failed");
  };
  return { arg, resolve, teardown };
}
