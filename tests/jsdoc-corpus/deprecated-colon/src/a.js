import { fnColon, fnPlain } from './lib.js';
export function use(foo) {
	fnColon();
	fnPlain();
	return foo({ ...fn });
}
