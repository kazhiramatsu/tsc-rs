export class R {
    static pool = 0;
    constructor(private readonly a: number, b: string) { void b; }
    private *items(): Iterable<number> { yield R.pool++; }
    async run() { return this.a; }
}
