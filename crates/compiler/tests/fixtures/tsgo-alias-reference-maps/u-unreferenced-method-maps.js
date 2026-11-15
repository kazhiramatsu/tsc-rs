var __decorate = (this && this.__decorate) || function (decorators, target, key, desc) {
    var c = arguments.length, r = c < 3 ? target : desc === null ? desc = Object.getOwnPropertyDescriptor(target, key) : desc, d;
    if (typeof Reflect === "object" && typeof Reflect.decorate === "function") r = Reflect.decorate(decorators, target, key, desc);
    else for (var i = decorators.length - 1; i >= 0; i--) if (d = decorators[i]) r = (c < 3 ? d(r) : c > 3 ? d(target, key, r) : d(target, key)) || r;
    return c > 3 && r && Object.defineProperty(target, key, r), r;
};
var U_1;
class Base {
    async overridden() { return 0; }
}
let U = class U extends Base {
    static { U_1 = this; }
    static { this.pool = U_1; }
    async plain() { return 1; }
    *generator() { yield 1; }
    async overridden() { return 1; }
    async typed(x = 1) { return x; }
};
U = U_1 = __decorate([
    dec
], U);
export { U };
//# sourceMappingURL=u-unreferenced-method-maps.js.map