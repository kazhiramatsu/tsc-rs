/**
 * @template {object} S
 * @template T
 * @overload
 * @param {S} store
 * @param {(value: S, set: (value: T) => void) => void} fn
 * @returns {T[]}
 */
/**
 * @template {object} S
 * @template T
 * @overload
 * @param {S} store
 * @param {(value: S) => T} fn
 * @returns {T[]}
 */
/**
 * @template {object} S
 * @template T
 * @param {S} store
 * @param {Function} fn
 * @returns {T[]}
 */
export function derived(store, fn) {
	return [];
}

/**
 * @template {object} S
 * @template T
 * @param {S} store
 * @param {(value: S) => T} fn
 * @returns {T[]}
 */
export function single(store, fn) {
	return [];
}
