/** @import { Value, Expression } from './types' */

/** @typedef {{ tag: string; reason: string; depth: number }} LastAutoClosedTag */

export class Parser {
	/** @type {LastAutoClosedTag | undefined} */
	last_auto_closed_tag;
	/** @param {string} name */
	check(name) {
		if (!this.last_auto_closed_tag || this.last_auto_closed_tag.tag !== name) {
			return 0;
		}
		return this.last_auto_closed_tag.depth;
	}
}

/**
 * @template V
 * @param {Value<V>} source
 * @param {V} value
 * @returns {V}
 */
export function internal_set(source, value) {
	if (!source.equals(value)) {
		return value;
	}
	return value;
}

/** @param {{ expression: Expression }} node */
export function let_directive(node) {
	return /** @type {Expression} */ (node.expression).type === 'ObjectExpression'
		? // @ts-expect-error
			node.expression.properties
		: // @ts-expect-error
			node.expression.elements;
}

/**
 * @template V
 * @overload
 * @param {() => V} get
 * @param {(v: V) => void} set
 * @returns {V}
 */
/**
 * @template V
 * @overload
 * @param {() => V} get
 * @returns {V}
 */
/**
 * Create a store from a function that returns state.
 *
 * ```ts
 * const store = toStore(() => count);
 * ```
 * @template V
 * @param {() => V} get
 * @param {(v: V) => void} [set]
 * @returns {V}
 */
export function toStore(get, set) {
	return get();
}

export { toStore as derived };
