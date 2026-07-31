// @noLib: true

// relpin p305: assignable source="(...args: [number]) => void" target="() => void"
declare var s: (...args: [number]) => void;
var t: () => void = s;
