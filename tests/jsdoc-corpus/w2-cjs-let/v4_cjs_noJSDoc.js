let A;
function load() {
	if (A === undefined) {
		A = { Identifier: 1 };
	}
}
class C {
	m() {
		load();
		return A.Identifier;
	}
}
module.exports = { C };
