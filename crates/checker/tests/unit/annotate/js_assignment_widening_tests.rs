use tsc_types::CompilerOptions;

use crate::{check_program, InputFile};

fn implicit_any_codes(options: CompilerOptions) -> Vec<u32> {
    check_program(
        &[InputFile::new("a.js".to_owned(), "class Module {}\nModule.prototype.identifier = undefined;\nModule.prototype.size = null;\n"
                .to_owned())],
        &options,
    )
    .diagnostics
    .iter()
    .filter(|diagnostic| matches!(diagnostic.code(), 7005 | 7043))
    .map(|diagnostic| diagnostic.code())
    .collect()
}

#[test]
fn js_nullable_assignment_reports_after_widening() {
    // With strict nullability disabled, getWidenedType turns the
    // nullable-only assignment into `any` before the nullable
    // filter, so no implicit-any suggestion is issued.
    assert_eq!(
        implicit_any_codes(CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            strict: Some(false),
            ..CompilerOptions::default()
        }),
        Vec::<u32>::new()
    );

    // The strict-null sibling remains nullable after widening and
    // therefore keeps the two suggestion-category 7043 rows.
    assert_eq!(
        implicit_any_codes(CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            strict: Some(false),
            strict_null_checks: Some(true),
            ..CompilerOptions::default()
        }),
        [7043, 7043]
    );
}

#[test]
fn method_assignment_inherits_the_base_property_type() {
    let result = check_program(
        &[InputFile::new("a.js".to_owned(), "class Base {\n  constructor() { this.p = 1; }\n}\nclass Derived extends Base {\n  m() { this.p = 1; }\n}\n"
                .to_owned())],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            no_implicit_any: Some(true),
            strict_null_checks: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert!(
        result
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code() != 2415),
        "{:?}",
        result.diagnostics
    );
}

#[test]
fn jsdoc_variadic_parameter_does_not_raise_the_minimum_arity() {
    let rows = |annotation: &str| {
        check_program(
            &[
                InputFile::new(
                    "a.js".to_owned(),
                    format!(
                        "/** @type {{{annotation}}} */\nconst foo = function (a, b, ...r) {{}};\n"
                    ),
                ),
                InputFile::new("b.ts".to_owned(), "foo(false, \"\");\n".to_owned()),
            ],
            &CompilerOptions {
                allow_js: true,
                check_js: Some(true),
                ..CompilerOptions::default()
            },
        )
        .diagnostics
        .iter()
        .filter(|diagnostic| matches!(diagnostic.code(), 2554 | 2555))
        .map(|diagnostic| diagnostic.code())
        .collect::<Vec<_>>()
    };

    assert_eq!(
        rows("function(boolean, string, ...*):void"),
        Vec::<u32>::new()
    );
    // A fixed third parameter is the control: without a rest
    // parameter tsc owns this as the exact-arity 2554 row, not
    // the "at least" 2555 rest-signature row.
    assert_eq!(rows("function(boolean, string, *):void"), [2554]);
}

#[test]
fn jsdoc_readonly_this_assignment_is_writable_in_class_and_js_constructors() {
    let text = "class C {\n  constructor(n) {\n    /** @readonly @type {number} */\n    this.y = n;\n  }\n  reset() { this.y = 0; }\n}\n";
    let rows = check_program(
        &[InputFile::new("a.js".to_owned(), text.to_owned())],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            strict: Some(false),
            ..CompilerOptions::default()
        },
    )
    .diagnostics
    .iter()
    .filter(|diagnostic| diagnostic.code() == 2540)
    .map(|diagnostic| diagnostic.start)
    .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [Some(
            text.find("this.y = 0").expect("non-constructor write") as u32 + "this.".len() as u32
        )]
    );

    // Exact jsdocReadonlyDeclarations corpus shape: `F` is a JS
    // constructor because its body declares a `this` property.
    // Readonly initialization is writable in both constructor
    // flavors; D's readonly parameter does not change that owner.
    let corpus_shape = "class C {\n    /** @readonly */\n    x = 6\n    /** @readonly */\n    constructor(n) {\n        this.x = n\n        /**\n         * @readonly\n         * @type {number}\n         */\n        this.y = n\n    }\n}\nnew C().x\n\nfunction F() {\n    /** @readonly */\n    this.z = 1\n}\n\nclass D {\n    constructor(/** @readonly */ x) {}\n}\n";
    let corpus_rows = check_program(
        &[InputFile::new(
            "jsdocReadonlyDeclarations.js".to_owned(),
            corpus_shape.to_owned(),
        )],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            strict: Some(false),
            target: Some(tsc_types::ScriptTarget::ES_NEXT.bits()),
            use_define_for_class_fields: Some(false),
            ..CompilerOptions::default()
        },
    )
    .diagnostics
    .iter()
    .filter(|diagnostic| diagnostic.code() == 2540)
    .map(|diagnostic| diagnostic.start)
    .collect::<Vec<_>>();
    assert_eq!(corpus_rows, Vec::<Option<u32>>::new());
}

#[test]
fn named_js_module_declarations_keep_their_module_face() {
    let result = check_program(
        &[
            InputFile::new("/mod1.js".to_owned(), "exports.a = { x: \"x\" };\nmodule[\"exports\"][\"d\"] = {};\nmodule[\"exports\"][\"d\"].e = 0;\n"
                    .to_owned()),
            InputFile::new("/mod2.js".to_owned(), "const mod1 = require(\"./mod1\");\nmod1.a;\nmod1.d;\nmod1.d.e;\n"
                    .to_owned()),
            InputFile::new("/expando.js".to_owned(), "const foo = {};\nfoo[\"baz\"] = {};\nfoo[\"baz\"][\"blah\"] = 3;\n"
                    .to_owned()),
        ],
        &CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            strict: Some(true),
            ..CompilerOptions::default()
        },
    );
    assert!(
        result
            .diagnostics
            .iter()
            .all(|diagnostic| !matches!(diagnostic.code(), 2339 | 7053)),
        "{:?}",
        result.diagnostics
    );
}
