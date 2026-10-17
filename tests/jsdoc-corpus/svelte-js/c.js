export class Boundary {
	/** @type {((anchor: object) => void)} */
	#children;
	constructor() {
		this.#children = (anchor) => {
			console.log(anchor);
		};
	}
}
