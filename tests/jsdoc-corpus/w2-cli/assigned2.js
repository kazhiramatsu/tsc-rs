/** @type {{ Identifier: number }} */
let NodeType;
/** @type {number} */
let Field;
function load() {
	if (NodeType === undefined) {
		({ NodeType, Field } = require("./data"));
	}
}
class C {
	m() {
		load();
		return NodeType.Identifier + Field;
	}
}
module.exports = { C };
