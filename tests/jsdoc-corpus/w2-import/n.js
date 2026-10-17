/** @import Foo, { Bar } from "./m" */
/**
 * @param {Foo} foo foo
 * @param {Bar} bar bar
 * @returns {number} n
 */
function use(foo, bar) {
	return foo.x + (bar ? 1 : 0);
}
const f = /** @type {unknown} */ (
	/**
	 * Handles the callback.
	 * @param {number[]} args args
	 * @returns {number} result
	 */
	(...args) => args.length
);
module.exports = { use, f };
