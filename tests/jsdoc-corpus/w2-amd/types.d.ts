export type Amd = false | { [index: string]: any };
export interface Options {
	amd?: Amd;
	commonjs?: boolean;
}
