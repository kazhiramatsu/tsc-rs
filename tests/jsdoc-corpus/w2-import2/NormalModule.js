/** @import Generator, { GenerateErrorFn } from "./Generator" */
class NormalModule {
	/**
	 * @param {Generator} generator generator
	 * @param {GenerateErrorFn} onError on error
	 * @returns {string} source
	 */
	build(generator, onError) {
		return generator.generate("x") + onError(new Error("e"), "n");
	}
}
module.exports = NormalModule;
