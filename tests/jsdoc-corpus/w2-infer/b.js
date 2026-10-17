/** @typedef {{ body: string[] }} Toplevel */
/**
 * @template T
 * @param {string} text the source
 * @param {(program: { kind: string }) => T} convert converts
 * @returns {{ result: T, table: number }} what convert made
 */
const readSource = (text, convert) => ({ result: convert({ kind: text }), table: 1 });
/**
 * @param {{ kind: string }} program program
 * @returns {Toplevel} toplevel
 */
const adopt = (program) => ({ body: [program.kind] });
const { result: toplevel, table } = readSource("x", (program) => adopt(program));
/** @type {number} */
const n = toplevel.body;
const { result: direct } = readSource("x", adopt);
/** @type {number} */
const m = direct.body;
module.exports = { n, m, table };
