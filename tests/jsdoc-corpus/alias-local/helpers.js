"use strict";
/** @typedef {import("./options").ContextOptions} ContextDependencyOptions */
/** @typedef {Partial<Omit<ContextDependencyOptions, "resource">>} PartialContextDependencyOptions */
/**
 * @param {string} request the request
 * @param {PartialContextDependencyOptions} contextOptions the options
 * @returns {string} dep
 */
const create = (request, contextOptions) => request + String(contextOptions.recursive);
/**
 * @param {string} expr the expression
 */
function parseObjectExpression(expr) {
	/** @typedef {Record<string, string | number | undefined>} Values */
	/** @type {Values} */
	const values = {};
	return { values, spread: expr.length > 0 };
}
function other() {
	return parseObjectExpression("x");
}
module.exports = { create, parseObjectExpression, other };
