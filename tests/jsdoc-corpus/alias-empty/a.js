/** @typedef {import("./t").Options} Options */
/** @param {Options} o o */
function f(o) {
	return o.a.dataUrl;
}
/**
 * @this {{ map: Record<string, string> }}
 * @type {(error: Error, name: string) => string}
 */
function g(error, name) {
	return this.map[name] + error.message;
}
module.exports = { f, g };
