import { deco } from './dep.js';

@deco
export class Q {
    static list(): Array<typeof Q> { return []; }
    static accepts(value: Q): value is Q { return true; }
}
