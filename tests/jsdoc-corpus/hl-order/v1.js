/**
 * @param {any} actual
 */
export function a(actual) {
	if (actual) throw new Error("x");
	return actual;
}
