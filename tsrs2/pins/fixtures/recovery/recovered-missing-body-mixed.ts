// @noLib: true

function modifiers(async this: C): number { return this.n; }
function initializer(this: C = new C()): number { return this.n; }
