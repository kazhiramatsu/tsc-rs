import { onlyOverload, onlyImpl, renamed } from './lib.js';
const a = { n: 1 };
export const x = onlyOverload(a);
export const y = onlyImpl(1);
export const z = renamed(a);
