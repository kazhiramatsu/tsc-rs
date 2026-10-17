/**
 * Defines the serializer middleware type used by this module.
 * @template [T=number]
 * @template [K=number]
 * @template [C=number]
 * @typedef {{ x: number }} SerializerMiddleware
 */

/**
 * Defines the serializer type used by this module.
 * @template D, S, C
 * @typedef {{ y: number }} Serializer
 */

/**
 * @template T
 * @template U
 * @param {number} n n
 * @returns {number} n
 */
function f(n) {
	return n;
}
module.exports = { f };
