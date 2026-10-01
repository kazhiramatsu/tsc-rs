use crate::{check_program_with_libs_at, CheckResult, InputFile};
use tsc_types::CompilerOptions;

fn focused_default_library() -> InputFile {
    InputFile::new(
        "/lib.d.ts",
        concat!(
            "interface IArguments {}\n",
            "interface Array<T> {}\n",
            "interface Object {}\n",
            "interface Function {}\n",
            "interface CallableFunction extends Function {}\n",
            "interface NewableFunction extends Function {}\n",
            "interface String {}\n",
            "interface Number {}\n",
            "interface Boolean {}\n",
            "interface RegExp {}\n",
        ),
    )
}

fn check(files: &[InputFile], stable_type_ordering: Option<bool>) -> CheckResult {
    let libs = [focused_default_library()];
    let options = CompilerOptions {
        strict: Some(true),
        stable_type_ordering,
        ..CompilerOptions::default()
    };
    check_program_with_libs_at(&libs, files, &options, "/")
}

fn messages(result: &CheckResult) -> Vec<String> {
    result
        .diagnostics
        .iter()
        .map(|diagnostic| {
            diagnostic
                .message_text()
                .as_str()
                .expect("message text is UTF-8")
                .to_owned()
        })
        .collect()
}

const BOX: &str = "interface Box<out A> { readonly box: (_: never) => A }\n";

/// tsc's default order is the type creation order; `stableTypeOrdering`
/// (tsc 6.0.3's preview of the TypeScript 7 order) ranks the two references
/// by their type arguments, `string` (TypeFlags 32) before `number` (64),
/// whichever was created first.
#[test]
fn stable_type_ordering_orders_union_members_by_content_not_by_creation() {
    for (declarations, default_order) in [
        (
            "declare const n: Box<number>\ndeclare const s: Box<string>\n",
            "Box<number> | Box<string>",
        ),
        (
            "declare const s: Box<string>\ndeclare const n: Box<number>\n",
            "Box<string> | Box<number>",
        ),
    ] {
        let text = format!(
            "{BOX}{declarations}declare const cond: boolean\nconst r: never = cond ? n : s\n"
        );
        let files = [InputFile::new("/main.ts", text)];

        let default = messages(&check(&files, None));
        assert_eq!(
            default,
            [format!(
                "Type '{default_order}' is not assignable to type 'never'."
            )],
            "default order for {declarations:?}"
        );

        let stable = messages(&check(&files, Some(true)));
        assert_eq!(
            stable,
            ["Type 'Box<string> | Box<number>' is not assignable to type 'never'.".to_owned()],
            "stable order for {declarations:?}"
        );
    }
}

/// The inference of `tapError`'s success type from the union
/// `Effect<A1, E1, R1> | Effect<void, never, never>` (effect's Stream.ts)
/// picks the first candidate. In the default order that is whichever type
/// was created first, so tsc itself errors when the `void` declaration is
/// checked after its use; the stable order ranks `void` before a type
/// parameter and the program checks the same way in either file order.
#[test]
fn stable_type_ordering_makes_inference_independent_of_file_order() {
    let effect = InputFile::new(
        "/effect.ts",
        concat!(
            "interface Variance<A, E, R> {\n",
            "  _A: (_: never) => A\n",
            "  _E: (_: never) => E\n",
            "  _R: (_: never) => R\n",
            "}\n",
            "interface Effect<out A, out E = never, out R = never> {\n",
            "  readonly effect: Variance<A, E, R>\n",
            "}\n",
            "interface Stream<out A, out E = never, out R = never> {\n",
            "  readonly stream: Variance<A, E, R>\n",
            "}\n",
            "declare function tapError<A, E, R, A2, E2, R2>(\n",
            "  self: Stream<A, E, R>,\n",
            "  f: (error: E) => Effect<A2, E2, R2>\n",
            "): Stream<A, E | E2, R | R2>\n",
        ),
    );
    let use_site = InputFile::new(
        "/use.ts",
        concat!(
            "const tapErrorIf = <A, E, R, A1, E1, R1>(\n",
            "  self: Stream<A, E, R>,\n",
            "  f: (e: E) => Effect<A1, E1, R1>,\n",
            "  p: (e: E) => boolean\n",
            "): Stream<A, E | E1, R | R1> =>\n",
            "  tapError(self, (error) => p(error) ? f(error) : voidEffect)\n",
        ),
    );
    let void_declaration = InputFile::new("/void.ts", "declare const voidEffect: Effect<void>\n");

    let void_first = [effect.clone(), void_declaration.clone(), use_site.clone()];
    let void_last = [effect, use_site, void_declaration];

    // tsc 6.0.3 without the option: clean with the declaration first, the
    // creation-order error with it last.
    assert_eq!(messages(&check(&void_first, None)), Vec::<String>::new());
    let default_void_last = messages(&check(&void_last, None));
    assert_eq!(default_void_last.len(), 1, "{default_void_last:?}");
    assert!(
        default_void_last[0].starts_with(
            "Type 'Effect<A1, E1, R1> | Effect<void, never, never>' is not assignable to type 'Effect<A1, E1, R1>'."
        ),
        "{default_void_last:?}"
    );

    // tsc 6.0.3 --stableTypeOrdering: clean in both orders.
    assert_eq!(
        messages(&check(&void_first, Some(true))),
        Vec::<String>::new()
    );
    assert_eq!(
        messages(&check(&void_last, Some(true))),
        Vec::<String>::new()
    );
}

/// createTypeofType (50137): the option sorts the `typeof` facts.
#[test]
fn stable_type_ordering_sorts_the_typeof_union() {
    let files = [InputFile::new(
        "/main.ts",
        "declare const value: unknown\nconst t: never = typeof value\n",
    )];
    assert_eq!(
        messages(&check(&files, None)),
        ["Type '\"string\" | \"number\" | \"bigint\" | \"boolean\" | \"symbol\" | \"undefined\" | \"object\" | \"function\"' is not assignable to type 'never'.".to_owned()]
    );
    assert_eq!(
        messages(&check(&files, Some(true))),
        ["Type '\"bigint\" | \"boolean\" | \"function\" | \"number\" | \"object\" | \"string\" | \"symbol\" | \"undefined\"' is not assignable to type 'never'.".to_owned()]
    );
}
