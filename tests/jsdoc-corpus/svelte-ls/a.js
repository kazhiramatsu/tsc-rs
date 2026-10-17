/** @import { Value, Source, Expression } from './types' */
import { g, CONSTANT } from './b.js';

/** @typedef {{ start?: number, end?: number }} NodeLike */

export class Parser {
	/** @type {null | { tag: string; depth: number }} */
	last = null;
	/** @param {string} name */
	check(name) {
		if (this.last !== null && this.last.tag !== name) {
			return this.last.depth;
		}
		return CONSTANT;
	}
	/** @returns {string} */
	generate() {
		return 'x';
	}
}

/**
 * @this {any}
 * @param {Record<string, unknown>} props
 * @param {Event} event
 */
export function bubble(props, event) {
	return this.props === props && event;
}

/**
 * @param {string} name
 * @returns {name is "a"}
 */
export function is_a(name) {
	return name === 'a';
}

/**
 * @template T
 * @param {any} actual
 * @param {T} expected
 * @returns {asserts actual is T}
 */
export function equal(actual, expected) {
	if (actual !== expected) throw new Error('x');
}

/**
 * Creates a store.
 *
 * ```ts
 * const s = toStore(() => 1);
 * ```
 * @template V
 * @param {() => V} get
 * @param {(v: V) => void} [set]
 * @returns {V}
 */
export function toStore(get, set) {
	return get();
}

/**
 * @template V
 * @param {Source<V>} source
 * @param {V} value
 */
export function internal_set(source, value) {
	if (!source.equals(value)) {
		return source.version;
	}
	return 0;
}

/** @param {{ expression: Expression }} node */
export function let_directive(node) {
	return /** @type {Expression} */ (node.expression).type === 'ObjectExpression'
		? node.expression.properties
		: node.expression.elements;
}

export function use() {
	const p = new Parser();
	const id = p.generate();
	return g(id.length) + toStore(() => 1);
}
