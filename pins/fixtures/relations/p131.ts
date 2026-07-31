// @noLib: true

// relpin p131: assignable source="new () => { a: number }" target="new () => {}"
declare var s: new () => { a: number };
var t: new () => {} = s;
