"use strict";
/** @typedef {import("./options").ContextOptions} ContextDependencyOptions */
/** @typedef {Partial<Omit<ContextDependencyOptions, "resource">>} PartialContextDependencyOptions */
/** @typedef {{ new (options: ContextDependencyOptions, range: number): object }} ContextDependencyConstructor */
/**
 * @template {ContextDependencyConstructor} T
 * @param {T} Dep the Dependency class
 * @param {string} request the request
 * @param {PartialContextDependencyOptions} contextOptions the options
 * @returns {InstanceType<T>} dep
 */
const create = (Dep, request, contextOptions) => /** @type {InstanceType<T>} */ (new Dep({ resource: request, ...contextOptions }, 1));
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
