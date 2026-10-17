/** @import { Source, Value } from './types' */

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
