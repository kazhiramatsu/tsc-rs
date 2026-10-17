/**
 * Throws an error.
 * @overload
 * @param {number} status status
 * @param {string} message message
 * @returns {never} nothing
 */
/**
 * Throws an error.
 * @overload
 * @deprecated Passing a body is deprecated
 * @param {number} status status
 * @param {{ message: string }} body body
 * @returns {never} nothing
 */
/**
 * @param {number} status status
 * @param {string | { message: string }} message message
 * @returns {never} nothing
 */
export function error(status, message) {
	throw new Error(String(status) + String(message));
}
/** @deprecated use error */
export function fail() {}
export function other() {
	return err;
}
