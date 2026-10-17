import { derived, single } from './lib.js';
const a = { n: 1 };
export const b = derived(a, (v) => v.n * 2);
export const c = single(a, (v) => v.n * 2);
