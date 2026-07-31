// @noLib: true

// relpin p314: assignable source="((a: number) => void) & ((a: string) => void)" target="(a: string) => void"
declare var s: ((a: number) => void) & ((a: string) => void);
var t: (a: string) => void = s;
