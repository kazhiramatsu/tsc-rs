/**
 * @template {{ keys(): IterableIterator<string> } | undefined} T
 */
export class SnapshotIterable {
	/** @param {T} value value */
	constructor(value) {
		this.value = value;
	}
}
/**
 * @template {{ name: string }} T
 * @param {T[]} items items
 * @returns {T | undefined} merged
 */
export const mergeToObject = (items) => items[0];
