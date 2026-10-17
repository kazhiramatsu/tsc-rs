class Hash {
	/**
	 * Calculates the digest
	 * @overload
	 * @returns {Buffer} digest
	 */
	/**
	 * Calculates the digest
	 * @overload
	 * @param {string} encoding encoding of the return value
	 * @returns {string} digest
	 */
	/**
	 * Calculates the digest
	 * @param {string=} encoding encoding of the return value
	 * @returns {string | Buffer} digest
	 */
	digest(encoding) {
		return encoding ? "" : Buffer.alloc(0);
	}
}
const h = new Hash();
const s = h.digest("hex");
const b = h.digest();
module.exports = { s, b };
