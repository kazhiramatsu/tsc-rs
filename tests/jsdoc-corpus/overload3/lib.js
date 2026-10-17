/**
 * @template T
 * @overload
 * @param {T} store
 * @returns {T[]}
 */
/**
 * @param {unknown} store
 * @returns {unknown[]}
 */
export function onlyOverload(store) { return []; }

/**
 * @overload
 * @param {number} store
 * @returns {number[]}
 */
/**
 * @template T
 * @param {T} store
 * @returns {T[]}
 */
export function onlyImpl(store) { return []; }

/**
 * @template T
 * @overload
 * @param {T} store
 * @returns {T[]}
 */
/**
 * @template U
 * @param {U} store
 * @returns {U[]}
 */
export function renamed(store) { return []; }
