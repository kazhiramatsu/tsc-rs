/** @typedef {(a: string, b: number) => string} Fn */
/** @type {Fn} */
function f(a, b) {
	return a.toUpperCase() + b.toFixed();
}
class C {
	/**
	 * @param {string | undefined} value v
	 * @returns {void}
	 */
	set name(value) {}
	get name() { return ""; }
}
for (const /** @type {string} */ s of ["a"]) {}
module.exports = { f, C };
