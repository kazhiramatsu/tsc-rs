interface A { a: number }
namespace N { export interface B<T> { b: T } }
interface I extends A, N.B<string> {}
class C implements A, N.B<number> { a = 1; b = 2; }
class D extends C implements I { b: string = ""; }
const E = class implements A, N.B<boolean> { a = 3; b = true; };
declare const x: { y?: { z: new () => A } };
interface G extends x.y?.z {}
class H implements x.y {}
