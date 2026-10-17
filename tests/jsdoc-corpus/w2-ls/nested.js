/**
 * @param {Object} state state
 * @param {{ kind: string }} state.ast ast
 * @param {string} state.name name
 * @returns {string} s
 */
const describe = (state) => state.ast.kind + state.name;

/**
 * @param {string} output output
 * @param {Object} options options
 * @param {string} options.context context
 * @param {number} options.targetProperties target properties
 * @returns {string} s
 */
const applyDefaults = (output, { context, targetProperties: tp }) => output + context + tp;
module.exports = { describe, applyDefaults };
