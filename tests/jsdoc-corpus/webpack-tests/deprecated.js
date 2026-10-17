export class C {
	/** @deprecated */
	gone = 1;
	constructor() {
		/**
		 * @deprecated
		 * @type {{ add: (item: string) => void }}
		 */
		this.deps = { add(item) {} };
		/** @deprecated */
		this.old = 1;
	}
	m() {
		return this.;
	}
}
