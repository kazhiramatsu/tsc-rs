/** @import { Derived, Effect, Source, Value } from '#client' */

var count_deps = 0;

/**
 * @template V
 * @param {Value<V>} source
 * @param {V} value
 * @param {Effect[] | null} [updated_during_traversal]
 * @returns {V}
 */
export function internal_set(source, value, updated_during_traversal = null) {
	return value;
}
