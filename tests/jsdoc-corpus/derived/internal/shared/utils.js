export function noop() {}
/** @param {Array<() => void>} fns */
export function run_all(fns) { fns.forEach((f) => f()); }
