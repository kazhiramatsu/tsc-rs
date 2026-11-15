import { deco } from './dep.js';

@deco
export class Q {
    static pool = 0;
    private *items(): Iterable<number> { yield Q.pool++; }
    async run() { return Q.pool; }
    private async other(x: number) { return x + Q.pool; }
    plain() { return 1; }
}
