/** @typedef {import("./types").Options} Options */
/**
 * @param {Options} options options
 * @returns {number} n
 */
function f(options) {
	/** @type {number} */
	const n = { mode: "auto", ...options };
	return n;
}
module.exports = { f };
