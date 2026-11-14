function dec(_: Function): void {}
@dec
export class A { static x = new A(); }
// tail
