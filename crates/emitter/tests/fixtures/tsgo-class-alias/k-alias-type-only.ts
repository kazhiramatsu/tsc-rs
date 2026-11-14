import { deco } from './dep.js';

@deco
export class K<T> {
    static cast<T>(): typeof K<T> { return this as typeof K<T>; }
}
