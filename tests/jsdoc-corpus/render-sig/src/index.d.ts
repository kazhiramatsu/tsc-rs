export interface Csp {
	nonce?: string;
}
export function render<Props extends Record<string, any> = Record<string, any>>(
	...args: {} extends Props
		? [
				component: (props: Props) => void,
				options?: {
					props?: Omit<Props, '$$slots'>;
					idPrefix?: string;
					csp?: Csp;
					transformError?: (error: unknown) => unknown | Promise<unknown>;
				}
			]
		: [
				component: (props: Props) => void,
				options: {
					props: Omit<Props, '$$slots'>;
					idPrefix?: string;
					csp?: Csp;
					transformError?: (error: unknown) => unknown | Promise<unknown>;
				}
			]
): string;
