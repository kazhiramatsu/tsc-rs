/** @type {{ Identifier: number }} */
let A;
/** @type {{ Identifier: number }} */
let B;
/** @type {{ Identifier: number }} */
let D;
function load() {
	if (A === undefined) {
		A = require("./data").NodeType;
	}
}
({ B } = require("./data2"));
D = require("./data").NodeType;
class C {
	m() {
		load();
		return A.Identifier + B.Identifier + D.Identifier;
	}
}
module.exports = { C };
