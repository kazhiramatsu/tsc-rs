/**
 * @template T
 * @param {T} value
 * @returns {T}
 */
export function clone(value) {
	if (typeof (/** @type {T & { toJSON?: any } } */ (value).toJSON) === 'function') {
		return /** @type {T & { toJSON(): any } } */ (value).toJSON();
	}
	return value;
}

/** @type {(options: { filename: string }) => 'injected' | 'external'} */
export const css = (options) => (options.filename ? 'injected' : 'external');
