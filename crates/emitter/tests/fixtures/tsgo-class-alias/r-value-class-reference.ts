import { deco } from './dep.js';

@deco
export class R {
    static make(): R { return new R(); }
}
