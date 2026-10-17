/** @typedef {{ type: 'Root', css: null }} Root */
export class Parser {
	/** @type {Root} */
	root;
	constructor() {
		this.root = {
			css: null,
			type: 'Root'
		};
	}
}
