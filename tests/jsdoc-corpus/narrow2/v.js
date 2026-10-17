/** @typedef {{ type: 'element', name: string }} Element */
/** @param {Element[]} fragment */
export function local(fragment) {
	/** @type {Element | undefined} */
	let e;
	e = { type: 'element', name: 'x' };
	fragment.push(e);
}
export class InCtor {
	/** @type {Element | undefined} */
	element;
	/** @type {Element[]} */
	fragment = [];
	constructor() {
		this.element = { type: 'element', name: 'x' };
		this.fragment.push(this.element);
	}
}
export class Cast {
	/** @type {Element | undefined} */
	element;
	/** @type {Element[]} */
	fragment = [];
	m() {
		this.element = /** @type {Element} */ ({ type: 'element', name: 'x' });
		this.fragment.push(this.element);
	}
}
export class Param {
	/** @type {Element | undefined} */
	element;
	/** @type {Element[]} */
	fragment = [];
	/** @param {Element} el */
	m(el) {
		this.element = el;
		this.fragment.push(this.element);
	}
}
