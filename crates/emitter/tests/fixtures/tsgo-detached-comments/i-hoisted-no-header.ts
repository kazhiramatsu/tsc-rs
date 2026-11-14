import { deco } from './dep.js';

// leading comment of the class
@deco
export class P { static make() { return new P(); } }
// trailing comment at the end
