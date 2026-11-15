declare const dec: any;
class Base { async overridden() { return 0; } }
@dec
export class U extends Base {
    static pool = U;
    async plain() { return 1; }
    private *generator() { yield 1; }
    public override async overridden() { return 1; }
    private async typed(x: number = 1) { return x; }
}
