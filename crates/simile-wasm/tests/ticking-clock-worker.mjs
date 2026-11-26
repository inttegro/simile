// Advance monotonic time on every read so Stack initialization cannot happen
// within one clock tick. Wall time and the production Worker stay unchanged.
let ticks = 0;
Object.defineProperty(performance, "now", { value: () => ++ticks });
await import("../dist/worker.js");
postMessage("ready");
