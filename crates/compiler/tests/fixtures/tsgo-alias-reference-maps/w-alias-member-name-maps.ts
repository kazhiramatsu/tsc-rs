import { deco } from './dep.js';
@deco
export class Field { Field = 1; }
@deco
export class Method { Method() {} }
@deco
export class Decorated { @deco Decorated() {} }
@deco
export class Async { async Async() {} }
