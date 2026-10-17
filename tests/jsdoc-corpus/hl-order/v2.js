/**
 * @template T
 * @param {any} actual
 * @param {T} expected
 * @returns {asserts actual is T}
 */
export function b(actual, expected) {
	if (actual !== expected) throw new Error("x");
}
