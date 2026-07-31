// @noLib: true

// relpin p134: assignable source="new () => void" target="() => void"
declare var s: new () => void;
var t: () => void = s;
