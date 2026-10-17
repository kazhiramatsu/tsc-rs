/**
 * @template T
 * @param {T & object} value
 */
export function proxy(value) {
	return /** @type {any[]} */ (value).length;
}
