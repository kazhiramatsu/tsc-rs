/** @typedef {{ [K in string]: K extends "a" ? number : string }} ByType */
class F {
	/**
	 * Returns a value.
	 * @template {string} T
	 * @param {T} type type
	 * @returns {ByType[T]} value
	 */
	get(type) {
		return /** @type {any} */ (null);
	}
}
const f = new F();
const x = f.get("a");
module.exports = { x };
