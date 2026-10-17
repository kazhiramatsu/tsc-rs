/** @type {{ Identifier: number }} */
let NodeType;
/** @type {number} */
let n;
class C {
	m() {
		return NodeType.Identifier + n;
	}
}
function g() {
	return n;
}
module.exports = { C, g };
