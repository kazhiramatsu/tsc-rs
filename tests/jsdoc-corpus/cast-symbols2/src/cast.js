/**
 * @template T
 * @param {T} value
 * @returns {T}
 */
export function clone(value) {
	if (typeof (/** @type {T & { toJSON?: any } } */ (value).toJSON) === 'function') {
		return /** @type {T & { toJSON(): any } } */ (value).toJSON();
	}
	const effect = /** @type {T & { nodes: string[] } } */ (value);
	return effect;
}

/**
 * @param {string[]} comments
 * @param {{ preserve_whitespace: number }} state
 * @returns {number}
 */
export const visitors = (comments, state) => comments.length + state.preserve_whitespace;

/**
 * @param {{ depth: number }} options
 */
export function walk(options) {
	return options.depth;
}
