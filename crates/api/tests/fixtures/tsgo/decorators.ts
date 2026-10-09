declare function dec(...args: any[]): any;
declare const arr: number[];
@dec(arr[0], "a" in {})
class C {
    @dec(arr[1]) m() {}
    @dec() accessor p = 1;
}
for (let i = dec("a" in {}); i; ) {}
