// @noLib: true
// @experimentalDecorators: false

declare let g: <T>(...args: any) => any;

{ @g<number> class C {} }

{ @g()<number> class C {} }
