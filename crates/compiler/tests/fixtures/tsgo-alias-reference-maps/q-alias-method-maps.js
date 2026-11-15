var __decorate = (this && this.__decorate) || function (decorators, target, key, desc) {
    var c = arguments.length, r = c < 3 ? target : desc === null ? desc = Object.getOwnPropertyDescriptor(target, key) : desc, d;
    if (typeof Reflect === "object" && typeof Reflect.decorate === "function") r = Reflect.decorate(decorators, target, key, desc);
    else for (var i = decorators.length - 1; i >= 0; i--) if (d = decorators[i]) r = (c < 3 ? d(r) : c > 3 ? d(target, key, r) : d(target, key)) || r;
    return c > 3 && r && Object.defineProperty(target, key, r), r;
};
var Q_1;
import { deco } from './dep.js';
let Q = class Q {
    static { Q_1 = this; }
    static { this.pool = 0; }
    *items() { yield Q_1.pool++; }
    async run() { return Q_1.pool; }
    async other(x) { return x + Q_1.pool; }
    plain() { return 1; }
};
Q = Q_1 = __decorate([
    deco
], Q);
export { Q };
//# sourceMappingURL=q-alias-method-maps.js.map