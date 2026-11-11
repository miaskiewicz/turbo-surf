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

const workerFixtures = new Map();
const workerTeardowns = [];

function declaration(value) {
  if (Array.isArray(value) && value.length === 2 && value[1] && typeof value[1] === "object") {
    return { def: value[0], options: value[1] };
  }
  return { def: value, options: {} };
}

async function teardownRecords(records, onError = () => {}) {
  const errors = [];
  for (const record of records.splice(0).reverse()) {
    record.release();
    try {
      await record.completion;
    } catch (error) {
      errors.push(error);
      onError(error);
    }
  }
  if (errors.length) throw new AggregateError(errors, "Fixture teardown failed");
}

export async function teardownWorkerFixtures() {
  try {
    await teardownRecords(workerTeardowns);
  } finally {
    workerFixtures.clear();
  }
}

async function resolveWorkerFixture(name, raw, initialize) {
  if (!workerFixtures.has(raw)) workerFixtures.set(raw, new Map());
  const cache = workerFixtures.get(raw);
  if (!cache.has(name)) cache.set(name, initialize());
  try {
    return await cache.get(name);
  } catch (error) {
    cache.delete(name);
    throw error;
  }
}

export async function openFixtures(
  baseDefs,
  extDefs,
  testInfo,
  onError = () => {},
  workerOnly = false,
) {
  const arg = {};
  const teardowns = [];
  const resolved = new Set();
  const resolving = new Set();
  const definition = (name, baseOnly) => {
    const overridden = !baseOnly && Object.hasOwn(extDefs, name);
    const defs = overridden ? extDefs : baseDefs;
    if (!Object.hasOwn(defs, name)) throw new Error(`Unknown fixture ${name}`);
    const base = declaration(baseDefs[name]);
    const current = declaration(defs[name]);
    const options = { ...base.options, ...current.options };
    const scope = options.scope ?? "test";
    if (!["test", "worker"].includes(scope)) throw new Error(`Unsupported fixture scope ${scope}`);
    return { ...current, options, scope, raw: defs[name], overridden };
  };
  const resolveFixture = async (name, baseOnly = false, requireWorker = workerOnly) => {
    const key = `${baseOnly ? "base:" : ""}${name}`;
    const entry = definition(name, baseOnly);
    if (resolving.has(key)) throw new Error(`Fixture dependency cycle at ${name}`);
    if (requireWorker && entry.scope !== "worker")
      throw new Error(`Worker fixture cannot depend on test fixture ${name}`);
    if (resolved.has(key)) return;
    resolving.add(key);
    try {
      const initialize = async () => {
        const values = {};
        for (const dependency of fixtureNames(entry.def)) {
          await resolveFixture(
            dependency,
            entry.overridden && dependency === name,
            entry.scope === "worker",
          );
          values[dependency] = arg[dependency];
        }
        const records = entry.scope === "worker" ? workerTeardowns : teardowns;
        await startFixture(name, entry.def, values, testInfo, records);
        return values[name];
      };
      arg[name] =
        entry.scope === "worker"
          ? await resolveWorkerFixture(name, entry.raw, initialize)
          : await initialize();
      resolved.add(key);
    } finally {
      resolving.delete(key);
    }
  };
  const resolve = async (names) => {
    for (const name of names) await resolveFixture(name);
  };
  const resolveAutomatic = async () => {
    for (const name of new Set([...Object.keys(baseDefs), ...Object.keys(extDefs)])) {
      const entry = definition(name, false);
      if (entry.options.auto && (!workerOnly || entry.scope === "worker"))
        await resolveFixture(name);
    }
  };
  const teardown = () => teardownRecords(teardowns, onError);
  return { arg, resolve, resolveAutomatic, teardown };
}
