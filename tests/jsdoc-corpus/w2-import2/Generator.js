/**
 * @typedef {(error: Error, name: string) => string} GenerateErrorFn
 */
class Generator {
	/**
	 * @param {string} name name
	 * @returns {string} source
	 */
	generate(name) {
		return name;
	}
}
module.exports = Generator;
