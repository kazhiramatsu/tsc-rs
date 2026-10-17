import { derived, writable } from './store/shared/index.js';
const a = writable(1);
const b = derived(a, (n) => n * 2);
const c = derived([a, writable(3)], ([x, y]) => x * y);
const evens = derived(a, (n, set) => { if (n % 2 === 0) set(n); });
export { b, c, evens };
