export class R {
    static { this.pool = 0; }
    constructor(a, b) {
        this.a = a;
        void b;
    }
    *items() { yield R.pool++; }
    async run() { return this.a; }
}
//# sourceMappingURL=r-undecorated-maps.js.map