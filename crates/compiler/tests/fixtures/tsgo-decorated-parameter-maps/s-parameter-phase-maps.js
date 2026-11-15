var __decorate = (this && this.__decorate) || function (decorators, target, key, desc) {
    var c = arguments.length, r = c < 3 ? target : desc === null ? desc = Object.getOwnPropertyDescriptor(target, key) : desc, d;
    if (typeof Reflect === "object" && typeof Reflect.decorate === "function") r = Reflect.decorate(decorators, target, key, desc);
    else for (var i = decorators.length - 1; i >= 0; i--) if (d = decorators[i]) r = (c < 3 ? d(r) : c > 3 ? d(target, key, r) : d(target, key)) || r;
    return c > 3 && r && Object.defineProperty(target, key, r), r;
};
var __param = (this && this.__param) || function (paramIndex, decorator) {
    return function (target, key) { decorator(target, key, paramIndex); }
};
var S_1;
import { deco, pdeco } from './dep.js';
let S = S_1 = class S {
    constructor(a = S_1, b = 1) {
        this.b = b;
    }
    method(value = S_1) { return value; }
};
__decorate([
    __param(0, pdeco)
], S.prototype, "method", null);
S = S_1 = __decorate([
    deco,
    __param(1, pdeco)
], S);
export { S };
//# sourceMappingURL=s-parameter-phase-maps.js.map