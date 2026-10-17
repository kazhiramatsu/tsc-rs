/** @typedef {{ type: 'element', name: string }} Element */
export class Template2 {
	/** @type {Element | undefined} */
	element;
	/** @type {Element[]} */
	fragment = [];
	/** @param {string} name */
	push_element(name) {
		this.element = { type: 'element', name };
		this.fragment.push(this.element);
	}
}
