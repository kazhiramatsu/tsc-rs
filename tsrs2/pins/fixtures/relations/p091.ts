// @noLib: true

// relpin p091: assignable source="{ m: () => number }" target="{ m(): number }"
declare var s: { m: () => number };
var t: { m(): number } = s;
