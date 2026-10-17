/**
 * @param {unknown} actual
 * @returns {asserts actual}
 */
export function d(actual) {
	if (!actual) throw new Error("x");
}
