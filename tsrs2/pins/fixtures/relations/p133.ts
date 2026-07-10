// @noLib: true

// relpin p133: assignable source="() => void" target="new () => void"
declare var s: () => void;
var t: new () => void = s;
