// @noLib: true

// relpin p132: assignable source="new () => {}" target="new () => { a: number }"
declare var s: new () => {};
var t: new () => { a: number } = s;
