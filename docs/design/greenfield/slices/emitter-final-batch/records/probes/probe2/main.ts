declare const a: { b: { c: number }, d: () => void, e: number[] };
declare const ns: { dec: any };
class C { x = a.b /* t1 */; }
const g = () => a.b /* t2 */;
async function h() { await a.b /* t3 */; }
const { p = a.b /* t4 */ } = a as any;
for (const v of a.e /* t5 */) { }
const s = `${a.b /* t6 */}`;
a.b?.c /* t7 */;
export const q = a.b /* t8 */;
enum E { A = a.e.length /* t9 */ }
namespace N { export const r = a.b /* t10 */; }
@ns.dec /* t11 */ class D { @ns.dec /* t12 */ m() { } }
