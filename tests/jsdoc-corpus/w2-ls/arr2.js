/** @typedef {{ id: number }} Chunk */
class Compilation {
	constructor() {
		/** @type {Set<Chunk>} */
		this.chunks = new Set();
		/** @type {Map<string, Chunk>} */
		this.named = new Map();
	}
	/**
	 * @param {Record<string, number>} options options
	 * @returns {number} n
	 */
	count(options) {
		const stack = [this.chunks];
		const keys = Object.keys(options);
		return keys.length + stack.length + this.chunks.size + this.named.size;
	}
}
module.exports = Compilation;
