interface I { method(): number; }
const Foo = class { static self: any; static { this.self = () => this; } }; export { Foo };
export const tail = 1;