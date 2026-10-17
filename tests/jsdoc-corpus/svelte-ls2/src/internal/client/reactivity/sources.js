/** @import { Value } from "#client" */

/**
 * @template V
 * @param {Value<V>} source
 * @param {V} value
 * @returns {V}
 */
export function internal_set(source, value) {
	if (!source.equals(value)) {
		return value;
	}
	return value;
}
