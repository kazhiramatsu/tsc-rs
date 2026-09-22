interface I { method(): number; }
function dec(value: any) { return value; } @dec export class Foo { static self: any; static { this.self = this; } }
export const tail = 1;
