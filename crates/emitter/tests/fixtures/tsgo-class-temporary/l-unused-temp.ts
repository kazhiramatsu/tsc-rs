import { pdeco } from './dep.js';

export class L {
    static #idPool = 0;
    readonly #id = `x-${L.#idPool++}`;
    constructor(@pdeco a: unknown) { void this.#id; }
}
