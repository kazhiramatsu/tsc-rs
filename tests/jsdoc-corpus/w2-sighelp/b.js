/**
 * @callback GetExportsFn
 * @param {string} data data
 * @param {number=} depth depth
 * @returns {string[] | null} exports
 */
/** @typedef {(data: string, depth?: number) => string[] | null} GetExportsTypedef */

/** @type {GetExportsFn} */
const getExportsA = (data, depth = 1) => (depth > 0 ? [data] : null);
/** @type {GetExportsTypedef} */
const getExportsB = (data, depth = 1) => (depth > 0 ? [data] : null);
module.exports = { getExportsA, getExportsB };
