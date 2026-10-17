/** @typedef {{ body: string[] }} Toplevel */
/**
 * @template T
 * @param {(program: { kind: string }) => T} convert converts
 * @returns {T} what convert made
 */
const run = (convert) => convert({ kind: "k" });
/**
 * @param {{ kind: string }} program program
 * @returns {Toplevel} toplevel
 */
const adopt = (program) => ({ body: [program.kind] });
const toplevel = run((program) => adopt(program));
/** @type {number} */
const n = toplevel.body;
module.exports = { n };
