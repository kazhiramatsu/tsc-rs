declare const dec: any;
@dec
export class T {
    static value = 1;
    method(x: boolean) {
        const a = x
            ? T
            : T;
        const b = x
            && T;
        const c = [
            T,
            T
        ];
        return /* before */ T /* after */.value + a.value + b.value + c.length;
    }
}
