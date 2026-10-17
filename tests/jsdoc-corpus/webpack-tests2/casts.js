/**
 * @template T
 * @param {() => T} load load
 * @returns {() => T} getter
 */
export const lazy = (load) => {
	/** @type {T | undefined} */
	let value;
	let len = 4;
	return () => {
		if (value === undefined) {
			value = load();
			/** @type {(() => T) | undefined} */
			(load) = undefined;
		}
		/** @type {number} */ (len)++;
		return /** @type {T} */ (value);
	};
};
