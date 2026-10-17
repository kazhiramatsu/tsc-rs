/**
 * @template {(...args: number[]) => number} T
 * @param {() => T} factory factory
 * @returns {T} function
 */
const lazyFunction = (factory) => {
	const f = /** @type {unknown} */ (
		/**
		 * Handles the callback.
		 * @param {number[]} args args
		 * @returns {number} result
		 */
		(...args) => factory()(...args) + args.length
	);
	return /** @type {T} */ (f);
};
module.exports = lazyFunction;
