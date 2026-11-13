import { waitForEmitterEvent } from "./event-waiter.mjs";

export class NativeLoadState {
  constructor(page) {
    this.page = page;
    this.reset();
  }
  reset() {
    this.states = new Set();
  }
  observe(kind, { emit = true } = {}) {
    const event = kind === "domContentLoaded" ? "domcontentloaded" : "load";
    const repeated = this.states.has(event);
    this.states.add(event);
    if (event === "load") this.states.add("domcontentloaded");
    if (!emit || repeated) return;
    this.page._emit ? this.page._emit(event, this.page) : this.page.emit(event, this.page);
  }
  async wait(state, options) {
    if (this.states.has(state)) return;
    await waitForEmitterEvent(this.page, state, options);
  }
}
