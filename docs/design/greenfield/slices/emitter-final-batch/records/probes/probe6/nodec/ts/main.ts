try {
    class Foo { public static func(): Foo { return new Foo(); } }
    Foo.func();
}
catch (e) {}