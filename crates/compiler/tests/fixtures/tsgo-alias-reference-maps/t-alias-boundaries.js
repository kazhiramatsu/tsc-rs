var __decorate = (this && this.__decorate) || function (decorators, target, key, desc) {
    var c = arguments.length, r = c < 3 ? target : desc === null ? desc = Object.getOwnPropertyDescriptor(target, key) : desc, d;
    if (typeof Reflect === "object" && typeof Reflect.decorate === "function") r = Reflect.decorate(decorators, target, key, desc);
    else for (var i = decorators.length - 1; i >= 0; i--) if (d = decorators[i]) r = (c < 3 ? d(r) : c > 3 ? d(target, key, r) : d(target, key)) || r;
    return c > 3 && r && Object.defineProperty(target, key, r), r;
};
var T_1;
let T = class T {
    static { T_1 = this; }
    static { this.value = 1; }
    method(x) {
        const a = x
            ? T_1 : T_1;
        const b = x
            && T_1;
        const c = [
            T_1,
            T_1
        ];
        return /* before */ T_1.value + a.value + b.value + c.length;
    }
};
T = T_1 = __decorate([
    dec
], T);
export { T };
//# sourceMappingURL=t-alias-boundaries.js.map