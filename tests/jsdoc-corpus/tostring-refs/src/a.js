/**
 * @typedef {{ code: string; message: string }} IDiag
 */

/** @implements {IDiag} */
export class C {
	code = 'x';
	message = 'm';
	toString() {
		return 'c';
	}
}

/** @param {unknown} obj */
export function is_date(obj) {
	return Object.prototype.toString.call(obj) === '[object Date]';
}

/** @param {C} c */
export function g(c) {
	return c.toString();
}
