/**
 * @template {Record<string, any>} Props
 * @param {(props: Props) => void} component
 * @param {{ props?: Omit<Props, '$$slots'>; idPrefix?: string; transformError?: (error: unknown) => unknown | Promise<unknown> }} [options]
 * @returns {string}
 */
export function render(component, options = {}) {
	return String(component) + String(options);
}
