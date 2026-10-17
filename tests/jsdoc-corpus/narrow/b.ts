type Element = { type: 'element', name: string };
export class Template {
	#element: Element | undefined;
	#fragment: Element[] = [];
	push_element(name: string) {
		this.#element = { type: 'element', name };
		this.#fragment.push(this.#element);
	}
}
