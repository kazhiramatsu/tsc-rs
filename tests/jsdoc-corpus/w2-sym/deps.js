class Resolver {
	constructor() {
		/** @type {{ add: (x: number) => void, remove: () => void }} */
		this.deps = {
			add(x) {},
			remove() {}
		};
		/** @type {Map<string, () => void>} */
		this.hooks = new Map();
	}
}
/** @type {{ run: (name: string) => void, stop: () => void }} */
const tasks = {
	run(name) {},
	stop() {}
};
module.exports = { Resolver, tasks };
