/**
 * @overload
 * @template {object} S
 * @template T
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
export function after(store, fn) { return []; }

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
export function before(store, fn) { return []; }

/**
 * @template T
 * @overload
 * @param {T} store
 * @returns {T[]}
 */
/**
 * @template T
 * @param {T} store
 * @returns {T[]}
 */
export function plain(store) { return []; }
