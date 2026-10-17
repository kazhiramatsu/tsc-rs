/** @type {{ Identifier: number }} */
let A;
/** @type {{ Identifier: number }} */
let D;
function load() {
	if (A === undefined) {
		A = { Identifier: 1 };
	}
}
D = { Identifier: 2 };
class C {
	m() {
		load();
		return A.Identifier + D.Identifier;
	}
}
export { C };
