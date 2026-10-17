/** @typedef {{ body: string[], endToken: number }} Toplevel */
/**
 * @template T
 * @param {string} text the source
 * @param {(program: { kind: string }, tokenAt: (pos: number) => number) => T} convert converts
 * @returns {{ result: T, table: number }} what convert made
 */
const readSource = (text, convert) => {
	/** @type {T | undefined} */
	let result;
	result = convert({ kind: text }, (pos) => pos);
	return { result: /** @type {T} */ (result), table: 1 };
};
/**
 * @param {{ kind: string }} program program
 * @param {(pos: number) => number} tokenAt tokenAt
 * @returns {Toplevel} toplevel
 */
const adopt = (program, tokenAt) => ({ body: [program.kind], endToken: tokenAt(0) });
const { result: toplevel, table } = readSource("x", (program, tokenAt) => adopt(program, tokenAt));
/** @type {number} */
const n = toplevel.body;
module.exports = { n, table };
