import { after, before, plain } from './lib.js';
const a = { n: 1 };
export const x = after(a, (v) => v.n * 2);
export const y = before(a, (v) => v.n * 2);
export const z = plain(a);
