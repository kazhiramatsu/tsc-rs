import { deco, pdeco } from './dep.js';
@deco
export class S {
    constructor(a = S, @pdeco private readonly b: number = 1) {}
    method(@pdeco value = S) { return value; }
}
