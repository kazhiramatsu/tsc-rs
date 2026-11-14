import { pdeco } from './dep.js';

export class S {
    #factory() { return S; }
    constructor(@pdeco a: unknown) { void this.#factory; }
}
