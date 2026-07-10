// @noLib: true

// relpin p271: comparable source="{ p: `é${string}` }" target="{ p: `e${string}` }"
declare var s: { p: `é${string}` };
var t = s as { p: `e${string}` };
