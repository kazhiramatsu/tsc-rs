/** @typedef {{ kind: string, body: Node[] }} Node */
/** @typedef {{ parent: Scope | null }} Scope */
/**
 * @param {{ statements: Node[], scope: Scope }} state state
 * @param {Node} previous previous
 * @returns {Node[]} nodes
 */
function reset(state, previous) {
	state.statements = /** @type {Node[]} */ (
		/** @type {Node[] | null} */ (null)
	);
	state.scope = /** @type {Scope} */ (/** @type {Scope | null} */ (null));
	previous.body = [
		.../** @type {Node[]} */ (previous.body),
		.../** @type {Node[]} */ (state.statements)
	];
	return previous.body;
}
module.exports = reset;
