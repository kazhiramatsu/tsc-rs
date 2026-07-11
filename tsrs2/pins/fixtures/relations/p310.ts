// @noLib: true

// relpin p310: assignable source="((a: string) => void) | ((a: number) => void)" target="(a: string & number) => void"
declare var s: ((a: string) => void) | ((a: number) => void);
var t: (a: string & number) => void = s;
