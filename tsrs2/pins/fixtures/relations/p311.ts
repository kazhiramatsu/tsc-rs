// @noLib: true

// relpin p311: assignable source="((a: string) => void) | ((a: number) => void)" target="(a: string) => void"
declare var s: ((a: string) => void) | ((a: number) => void);
var t: (a: string) => void = s;
