/**
 * @typedef {Object} Context
 * @property logger
 * @property {string[]} rawArguments
 * @property argv
 */

/**
 * @param {string} text the text
 * @param {Object} [options] the options
 * @param options.filePath the path
 * @param {boolean} [options.warn] warn
 * @returns {string} result
 */
function lint(text, options = {}) {
	return text + (options.filePath || "") + (options.warn ? "!" : "");
}

/** @type {Context} */
const context = { logger: 1, rawArguments: [], argv: 2 };
module.exports = { lint, context };
