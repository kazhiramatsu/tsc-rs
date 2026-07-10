// @noLib: true

// relpin p258: comparable source="(x: number) => void" target="(x: string) => void"
declare var s: (x: number) => void;
var t = s as (x: string) => void;
