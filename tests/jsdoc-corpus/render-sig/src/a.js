import { render } from './index.js';

/** @param {(props: Record<string, any>) => void} component */
export function go(component) {
	return render(component, { idPrefix: 'x' });
}
