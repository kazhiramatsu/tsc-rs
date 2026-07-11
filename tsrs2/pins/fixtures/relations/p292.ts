// @noLib: true

// relpin p292: assignable source="1" target="{ toFixed(digits: number): string }"
interface Number { toFixed(digits: number): string }
declare var s: 1;
var t: { toFixed(digits: number): string } = s;
