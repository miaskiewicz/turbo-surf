const drains = new WeakMap();

function current(page) {
  const state = drains.get(page);
  return state?.session === page._session ? state : undefined;
}

export function throwBindingDrainError(page) {
  const state = current(page);
  if (!state?.failure) return;
  const { error } = state.failure;
  state.failure = undefined;
  throw error;
}

export function scheduleBindingDrain(page) {
  throwBindingDrainError(page);
  if (current(page)?.pending) return;
  const state = { session: page._session, pending: true };
  drains.set(page, state);
  // Input dispatch finishes at the event checkpoint. Keep servicing host calls
  // independently, without treating their guest Promises as input completion.
  page
    ._drainBindings()
    .catch((error) => {
      state.failure = { error };
    })
    .finally(() => {
      state.pending = false;
    });
}
