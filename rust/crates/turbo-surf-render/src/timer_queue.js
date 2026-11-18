const __timers = [];
Object.setPrototypeOf(__timers, null);
const __timerRawPush = Array.prototype.push.bind(__timers);
const __timerRawSplice = Array.prototype.splice.bind(__timers);
const __timerFindIndex = Array.prototype.findIndex.bind(__timers);
const __timerFinite = Number.isFinite;
// Bursts normally arrive in deadline order; once uncertain, retain the full scanner
// until the queue empties. Removing records cannot establish a new ordering.
let __timerOrdered = true;
const __timerEligible = (timer) => __timerFinite(timer.due) && __timerFinite(timer.sequence);
const __timerBefore = (a, b) => a.due < b.due || (a.due === b.due && a.sequence < b.sequence);
const __timerPush = (timer) => {
  const last = __timers[__timers.length - 1];
  if (!__timerEligible(timer) || (last && __timerBefore(timer, last))) __timerOrdered = false;
  return __timerRawPush(timer);
};
const __timerSplice = (index, count, timer) => {
  if (timer !== undefined) {
    const first = __timers[0];
    if (
      index !== 0 ||
      count !== 0 ||
      !__timerEligible(timer) ||
      (first && __timerBefore(first, timer))
    )
      __timerOrdered = false;
  }
  const removed =
    timer === undefined ? __timerRawSplice(index, count) : __timerRawSplice(index, count, timer);
  if (!__timers.length) __timerOrdered = true;
  return removed;
};
const __timerClearQueue = () => {
  __timers.length = 0;
  __timerOrdered = true;
};
const __nextTimerIndex = () => {
  if (__timerOrdered) return 0;
  let index = 0;
  for (let i = 1; i < __timers.length; i++) {
    const timer = __timers[i],
      best = __timers[index];
    if (timer.due < best.due || (timer.due === best.due && timer.sequence < best.sequence))
      index = i;
  }
  return index;
};
// A timer task's nested timers count from the latest due time among the timers already
// due with it (within the same millisecond of its own due time): in Chrome the parent
// runs no earlier than those siblings were due, so a nested zero-delay timer queues
// behind an equal-delay sibling set slightly later by the live clock (turbo-surf#118).
// Rendering frames are not siblings (turbo-surf#101); only queued dues are read, never
// the live clock, so the order stays deterministic.
const __orderedTaskBase = (due) => {
  let low = 0,
    high = __timers.length;
  while (low < high) {
    const middle = (low + high) >>> 1;
    if (__timers[middle].due < due + 1) low = middle + 1;
    else high = middle;
  }
  for (let i = low - 1; i >= 0; i--) {
    const timer = __timers[i];
    if (timer.due <= due) break;
    if (!timer.rendering) return timer.due;
  }
  return due;
};
const __taskBaseFor = (due) => {
  if (__timerOrdered && __timerFinite(due)) return __orderedTaskBase(due);
  let base = due;
  for (let i = 0; i < __timers.length; i++) {
    const t = __timers[i];
    if (!t.rendering && t.due > base && t.due < due + 1) base = t.due;
  }
  return base;
};
