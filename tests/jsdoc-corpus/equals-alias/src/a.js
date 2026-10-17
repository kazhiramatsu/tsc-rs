/** @import { Value } from './types' */

/** @type {import('./types').Equals} */
export function equals(value) {
	return value === this.v;
}

/**
 * @template V
 * @param {V} v
 * @returns {Value<V>}
 */
export function source(v) {
	return { f: 0, wv: 0, equals, v };
}

/**
 * @template V
 * @param {Value<V>} source
 * @param {V} value
 * @returns {V}
 */
export function internal_set(source, value) {
	if (!source.equals(value)) {
		source.v = value;
	}
	return value;
}
