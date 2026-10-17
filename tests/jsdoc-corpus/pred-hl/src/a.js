/**
 * @param {any} obj
 * @returns {obj is Date}
 */
export function plain(obj) {
	return !!obj;
}

/**
 * @param {any} obj
 * @returns {asserts obj is Date}
 */
export function asserts_is(obj) {
	if (!obj) throw new Error('x');
}

/**
 * @param {any} obj
 * @returns {asserts obj}
 */
export function asserts_plain(obj) {
	if (!obj) throw new Error('x');
}

/**
 * @template T
 * @param {any} obj
 * @param {T} other
 * @returns {obj is T}
 */
export function plain_template(obj, other) {
	return obj === other;
}

/**
 * @param {any} obj
 * @returns {obj is Date} whether
 */
export function plain_described(obj) {
	return !!obj;
}
