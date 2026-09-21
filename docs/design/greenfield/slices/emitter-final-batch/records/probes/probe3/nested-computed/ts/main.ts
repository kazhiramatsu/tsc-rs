interface I { method(): number; }
const Foo = class { static key = 'member'; static nested = class { [this.key]() {} }; }; export { Foo };
export const tail = 1;