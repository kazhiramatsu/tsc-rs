function decorator() { return (target: new (...args: any[]) => any) => {} }
try {
    @decorator()
    class Foo { public static func(): Foo { return new Foo(); } }
    Foo.func();
}
catch (e) {}