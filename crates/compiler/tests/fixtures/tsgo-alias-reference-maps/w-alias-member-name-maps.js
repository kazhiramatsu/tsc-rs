var __decorate = (this && this.__decorate) || function (decorators, target, key, desc) {
    var c = arguments.length, r = c < 3 ? target : desc === null ? desc = Object.getOwnPropertyDescriptor(target, key) : desc, d;
    if (typeof Reflect === "object" && typeof Reflect.decorate === "function") r = Reflect.decorate(decorators, target, key, desc);
    else for (var i = decorators.length - 1; i >= 0; i--) if (d = decorators[i]) r = (c < 3 ? d(r) : c > 3 ? d(target, key, r) : d(target, key)) || r;
    return c > 3 && r && Object.defineProperty(target, key, r), r;
};
var Field_1, Method_1, Decorated_1, Async_1;
import { deco } from './dep.js';
let Field = Field_1 = class Field {
    constructor() {
        this.Field_1 = 1;
    }
};
Field = Field_1 = __decorate([
    deco
], Field);
export { Field };
let Method = Method_1 = class Method {
    Method_1() { }
};
Method = Method_1 = __decorate([
    deco
], Method);
export { Method };
let Decorated = Decorated_1 = class Decorated {
    Decorated_1() { }
};
__decorate([
    deco
], Decorated.prototype, "Decorated", null);
Decorated = Decorated_1 = __decorate([
    deco
], Decorated);
export { Decorated };
let Async = Async_1 = class Async {
    async Async_1() { }
};
Async = Async_1 = __decorate([
    deco
], Async);
export { Async };
//# sourceMappingURL=w-alias-member-name-maps.js.map