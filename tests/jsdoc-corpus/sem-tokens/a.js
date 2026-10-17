/**
 * @typedef {Object} Last
 * @property {string} tag
 * @property {number} depth
 */

export class Parser {
	/** @type {Last | undefined} */
	last;
	/** @param {string} name */
	check(name) {
		if (!this.last || this.last.tag !== name) {
			return 0;
		}
		return this.last.depth;
	}
}
