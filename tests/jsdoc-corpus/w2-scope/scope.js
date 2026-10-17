class Scope {
	/**
	 * @param {Scope | undefined} upper upper
	 * @param {boolean} isVarScope var scope
	 */
	constructor(upper, isVarScope) {
		/** @type {Scope} the nearest enclosing scope `var` hoists to */
		this.variableScope = isVarScope
			? this
			: /** @type {Scope} */ (upper).variableScope;
		/** @type {number} */
		this.depth = upper ? upper.depth + 1 : 0;
	}
}
module.exports = Scope;
