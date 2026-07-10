// @noLib: true

// relpin p090: assignable source="{ m(): number }" target="{ m: () => number }"
declare var s: { m(): number };
var t: { m: () => number } = s;
