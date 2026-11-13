const decoded = (value) => (typeof value === "string" ? JSON.parse(value) : value);

// One native actor owns multiple genuine browsing contexts. Only this hub consumes
// its lifecycle and resource queues; Page aliases address a context, not a runtime.
export class LiveActorHub {
  constructor(native, identity, rootPage, callbacks) {
    this.native = native;
    this.actor = identity.actor;
    this.root = identity.root;
    this.capability = identity.capability?.kind ?? "pending";
    this.capabilityReason = identity.capability?.reason;
    this.heldEvents = [];
    this.capability = identity.capability?.kind ?? "pending";
    this.capabilityReason = identity.capability?.reason;
    this.heldEvents = [];
    this.rootPage = this.root == null ? rootPage : null;
    this.pages = new Map(this.root == null ? [] : [[this.root, rootPage]]);
    this.callbacks = callbacks;
    if (this.root != null) this.callbacks.root = undefined;
    this.retiredPages = new Map();
    this.published = new Set(this.root == null ? [] : [this.root]);
    this.retired = new Set();
    this.info = new Map();
    this.requests = new Map();
    this.active = true;
    this.opening = identity.opening ?? false;
    this.ready = new Promise((resolve, reject) => {
      this._resolveReady = resolve;
      this._rejectReady = reject;
    });
    this.ready.catch(() => {});
    if (!this.opening) this._resolveReady();
  }

  page(target) {
    return this.pages.get(target) ?? this.retiredPages.get(target)?.deref();
  }

  initialize(state) {
    this.pending = Promise.resolve()
      .then(() => this.applyState(state))
      .finally(() => {
        this.pending = null;
      });
    return this.pending;
  }

  drain() {
    if (this.pending) return this.pending;
    this.pending = this._drain().finally(() => {
      this.pending = null;
    });
    return this.pending;
  }

  async _drain() {
    if (!this.active) return;
    const state = decoded(await this.native.liveAuxiliaryState(this.actor));
    await this.applyState(state);
    if (!this.active || this.capability !== "supported") return;
    const raw = await this.native.liveNetworkLog(this.actor);
    if (!raw) return;
    const batch = decoded(raw);
    const previous = new Map();
    for (const entry of Array.isArray(batch) ? batch : batch.entries) {
      const page = this._networkPage(entry);
      if (!page) continue;
      const hop = await this.callbacks.network(page, entry, previous.get(page));
      if (hop !== undefined) previous.set(page, hop);
    }
    if (batch.revision != null) this.native.liveNetworkLogAcknowledge(this.actor, batch.revision);
  }

  _networkPage(entry) {
    const request = entry.id == null ? null : String(entry.id);
    if (entry.event === "responsebody") {
      const page = this.requests.get(request);
      this.requests.delete(request);
      return page;
    }
    if (entry.event === "networkoverflow") {
      for (const page of this.pages.values()) this.callbacks.network(page, entry);
      return null;
    }
    const target = entry.context?.frame?.pageId;
    const page = this.pages.get(target);
    if (!page) throw new Error("Native actor event has no owned Page identity");
    if (entry.bodyPending && request != null) this.requests.set(request, page);
    return page;
  }

  _rememberPages(state) {
    for (const info of state.pages) {
      if (this.info.get(info.target)?.closed && !info.closed) continue;
      this.info.set(info.target, info);
    }
  }

  _acceptCapability(state) {
    const capability = state.capability?.kind ?? this.capability;
    if (state.capability != null && !state.capability.kind)
      throw new Error("Invalid native auxiliary capability descriptor");
    if (state.capability?.reason) this.capabilityReason = state.capability.reason;
    if (!["pending", "supported", "unsupported"].includes(capability))
      throw new Error("Invalid native auxiliary capability");
    if (this.capability !== "pending" && capability !== this.capability)
      throw new Error("Native root capability changed");
    const unsupported = this.capability === "pending" && capability === "unsupported";
    this.capability = capability;
    if (unsupported) {
      this.callbacks.unsupported?.();
      this.native.liveNetworkLogWake?.(this.actor);
    }
  }

  _acceptIdentity(state) {
    if (state.actor !== this.actor || (this.root != null && state.root !== this.root))
      throw new Error("Native auxiliary actor identity changed");
    if (state.revision != null && BigInt(state.revision) >= BigInt(this.revision ?? "0"))
      this.revision = state.revision;
    this._acceptCapability(state);
    if (state.opening === false && !state.closed) {
      this.opening = false;
      this._resolveReady();
    }
  }

  _acceptRoot(target) {
    if (this.root != null || target == null) return;
    this.root = target;
    this.pages.set(this.root, this.rootPage);
    this.published.add(this.root);
    this.callbacks.root?.(this.root);
    this.callbacks.root = undefined;
    this.rootPage = null;
  }

  async applyState(state) {
    this._acceptIdentity(state);
    const overflow = state.events.find((event) => event.kind === "overflow");
    if (overflow) throw new Error(overflow.message);
    if (this.capability !== "supported") {
      this.heldEvents.push(...state.events);
      if (
        this.capability === "unsupported" &&
        (state.root != null || state.pages.length || this.heldEvents.length)
      )
        throw new Error("Unsupported root published auxiliary identities");
      if (state.closed) this._retireAll();
      return;
    }
    this._acceptRoot(state.root);
    this._rememberPages(state);
    const events = [...this.heldEvents.splice(0), ...state.events];
    for (const event of events) await this._event(event);
    if (state.closed) this._retireAll();
  }

  async _event(event) {
    if (event.kind === "overflow") throw new Error(event.message);
    if (event.kind === "created") {
      if (this.published.has(event.target)) return;
      const info = this.info.get(event.target);
      if (!info) throw new Error("Native auxiliary creation has no owned identity");
      const alias = info.closed ? null : this.native.liveAuxiliaryAdopt(this.actor, event.target);
      const page = await this.callbacks.create({ ...info, ...event, closed: info.closed }, alias);
      this.pages.set(event.target, page);
      this.published.add(event.target);
      this.callbacks.created(page, event);
      return;
    }
    const page = this.page(event.target);
    if (!page) return;
    if (event.kind === "closed") await this.closeTarget(event.target);
    if (event.kind === "navigated") await this.callbacks.navigated(page, event);
    if (["domContentLoaded", "load"].includes(event.kind)) this.callbacks.lifecycle(page, event);
  }

  retire(target) {
    if (this.retired.has(target)) return;
    const page = this.page(target);
    const finalState = page ? this.callbacks.closed(page) : null;
    this.retired.add(target);
    if (page) this.retiredPages.set(target, new WeakRef(page));
    this.pages.delete(target);
    return finalState;
  }

  async closeTarget(target) {
    const finalState = this.retire(target);
    if (finalState) await this.applyState(decoded(finalState));
  }

  _retireAll() {
    this.active = false;
    this._rejectReady(new Error("Native actor closed during initialization"));
    if (this.root == null && !this.retired.has(null)) {
      this.retired.add(null);
      this.callbacks.closed(this.rootPage);
    }
    for (const target of this.pages.keys()) this.retire(target);
    this.pages.clear();
    this.retiredPages.clear();
    this.info.clear();
    this.rootPage = null;
    this.requests.clear();
    this.callbacks.actorClosed?.(this.actor);
  }

  _bindingPages() {
    return [...this.pages.values()].filter((page) => !page._closed && page._session != null);
  }

  _bindingDeadline(deadline, cancelOnTimeout) {
    if (Date.now() < deadline) return;
    const error = Object.assign(new Error("Timeout exceeded while waiting for host bindings"), {
      name: "TimeoutError",
    });
    if (cancelOnTimeout) for (const page of this.pages.values()) page._routeLifetime?.abort(error);
    throw error;
  }

  async _takeActorBindings(state) {
    let count = 0;
    for (const page of this._bindingPages()) {
      const handlers = page._mergedBindings();
      if (!handlers.size) continue;
      const calls = await page._takeBindingCalls();
      count += calls.length;
      for (const call of calls) {
        const entry = handlers.get(call.name);
        if (!entry) continue;
        state.changed.add(page);
        const task = page
          ._settleBindingCall(call, entry)
          .catch((error) => {
            state.failed = true;
            state.failure = error;
          })
          .finally(() => state.pending.delete(task));
        state.pending.add(task);
      }
    }
    return count;
  }

  async _advanceActorBindings(state, collect) {
    state.advanced = true;
    for (const page of state.changed) {
      if (page._closed || page._session == null || page._contentCollecting) continue;
      await page._collectSession(page._session, "globalThis.__RESULT = '';", collect);
    }
  }

  async _bindingStep(state, collect) {
    const count = await this._takeActorBindings(state);
    if (count && ++state.rounds > 100) throw new Error("Exposed binding call budget exhausted");
    if (count) state.advanced = false;
    if (!state.pending.size && !count) {
      if (!state.changed.size || state.advanced) return false;
      await this._advanceActorBindings(state, collect);
      return true;
    }
    if (state.pending.size)
      await Promise.race([...state.pending, new Promise((resolve) => setTimeout(resolve, 1))]);
    if (state.failed) throw state.failure;
    return true;
  }

  async drainBindings(deadline = Infinity, cancelOnTimeout = true, collect = false) {
    if (this.bindingDrain || this.opening) return;
    this.bindingDrain = true;
    const state = { pending: new Set(), changed: new Set(), advanced: false, rounds: 0 };
    try {
      while (this.active) {
        this._bindingDeadline(deadline, cancelOnTimeout);
        await this.drain();
        if (!(await this._bindingStep(state, collect))) break;
      }
      for (const page of state.changed) if (!page._closed) await page._refreshFromSession(true);
    } finally {
      this.bindingDrain = false;
    }
  }

  start() {
    if (this.started || !this.active || this.capability === "unsupported") return;
    this.started = true;
    this.lifecycleTask = this._watchLifecycle().catch((error) => this._failed(error));
    this._watchNetwork().catch((error) => this._failed(error));
  }

  async _watchLifecycle() {
    while (this.active && this.capability !== "unsupported") {
      const signal = decoded(await this.native.liveAuxiliaryWait(this.actor, this.revision ?? "0"));
      if (!this.active) return;
      if (signal.closed) {
        if (this.closing) {
          this.closingRecovery = signal.finalState;
          return;
        }
        if (signal.finalState) await this.applyState(signal.finalState);
        this._retireAll();
        return;
      }
      await this.drain();
    }
  }

  async _watchNetwork() {
    let wake = "0";
    while (this.active && this.capability !== "unsupported") {
      const signal = decoded(await this.native.liveNetworkLogWait(this.actor, wake));
      if (!this.active || this.capability === "unsupported" || signal.closed) return;
      wake = signal.wake;
      await this.drain();
    }
  }

  _failed(error) {
    if (this.active && !this.closing) this.callbacks.failure(error);
  }

  releaseRoot(error = new Error("Root execution session replaced")) {
    if (this.capability === "supported")
      throw new Error("Cannot release an owned auxiliary actor as a legacy root");
    this.active = false;
    this._rejectReady(error);
    this.rootPage = null;
    this.callbacks.actorClosed?.(this.actor);
  }

  async close() {
    if (this.closing) return this.closing;
    this.closing = this._closeActor();
    return this.closing;
  }

  async _closeActor() {
    const finalState = decoded(this.native.liveActorClose(this.actor));
    if (finalState) this._rememberPages(finalState);
    const pending = this.pending;
    if (pending) await pending.catch(() => {});
    if (!finalState) await this.lifecycleTask;
    try {
      const receipt = finalState ?? this.closingRecovery;
      if (receipt) await this.applyState(receipt);
    } finally {
      this._retireAll();
    }
  }
}
