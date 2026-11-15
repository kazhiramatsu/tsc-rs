import { pdeco } from './dep.js';

export class P {
    constructor(
        @pdeco private readonly a: number,
        @pdeco b: string,
        private readonly c: number,
        @pdeco private readonly d: number = 1,
    ) { void b; }
    get v() { return this.a + this.c + this.d; }
}
