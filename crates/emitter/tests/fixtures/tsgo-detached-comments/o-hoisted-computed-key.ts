/* Header before a computed-key temporary. */

import { deco } from './dep.js';

class Computed { @deco [Symbol.iterator]() {} }
// EOF after a computed-key temporary.
