//! A node printed for an insertion (tsgo `PrintAndPositionNode`,
//! `CreateSyntheticSourceFile`) and formatted where it goes
//! (`handleFormatNodeForInsertion`): the positions of the printed nodes and
//! lists, the text without a source file, and the formatted insertions, all
//! as tsgo gives them (taken from tsgo with a Go probe).

use tsc_emitter::{print_and_position_node, NewLineKind, PositionedNode};
use tsc_ls::lsutil::UserPreferences;
use tsc_ls::tree::{kind_name, Tree, View};
use tsc_syntax::{
    for_each_child, for_each_child_array, parse_source_file, skip_trivia,
    ExternalModuleIndicatorOptions, LanguageVariant, NodeArena, NodeData, NodeId, ParseOptions,
    ScriptKind, SourceFile, SyntaxKind,
};

/// The node as the API decodes it: no positions, no parents, no trailing
/// commas.
fn synthesize(arena: &mut NodeArena, node: NodeId) {
    let mut children = Vec::new();
    for_each_child(arena, arena.node(node), |child| {
        children.push(child);
        false
    });
    let mut arrays = Vec::new();
    for_each_child_array(arena.node(node), |array| {
        arrays.push(array);
        false
    });
    for array in arrays {
        let record = arena.node_array_mut(array);
        record.pos = u32::MAX;
        record.end = u32::MAX;
        record.has_trailing_comma = false;
    }
    let record = arena.node_mut(node);
    record.pos = u32::MAX;
    record.end = u32::MAX;
    record.parent = None;
    for child in children {
        synthesize(arena, child);
    }
}

/// The first statement of `text`, decoded, printed and positioned.
fn print_and_position(
    text: &str,
    script_kind: ScriptKind,
    options: (NewLineKind, usize),
    variant: LanguageVariant,
) -> PositionedNode {
    let file_name = if script_kind == ScriptKind::Tsx {
        "/n.tsx"
    } else {
        "/n.ts"
    };
    let source = parse_source_file(
        file_name,
        text.to_owned(),
        ParseOptions {
            script_kind,
            ..ParseOptions::default()
        },
        None,
    );
    let NodeData::SourceFile(data) = &source.arena.node(source.root).data else {
        unreachable!()
    };
    let statement = source.arena.node_array(data.statements.unwrap()).nodes[0];
    // The decoder keeps a single-quoted literal's quote as a flag.
    let single_quoted: Vec<NodeId> = source
        .arena
        .node_ids()
        .filter(|&id| {
            let node = source.arena.node(id);
            node.kind == SyntaxKind::StringLiteral
                && source.text()[skip_trivia(source.text(), node.pos as usize)..].starts_with('\'')
        })
        .collect();
    let mut arena = source.arena.detached_clone();
    synthesize(&mut arena, statement);
    let decoded = SourceFile::from_tree(
        "",
        None,
        arena,
        statement,
        LanguageVariant::Standard,
        ScriptKind::Unknown,
        false,
        ExternalModuleIndicatorOptions::default(),
    );
    print_and_position_node(
        &decoded,
        statement,
        &single_quoted,
        options.0,
        options.1,
        variant,
    )
    .unwrap()
}

/// tsgo's tree of a positioned node, each node and list with its range.
fn dump(tree: &Tree<'_>, view: View, depth: usize, out: &mut String) {
    let (pos, end) = tree.range(view);
    let name = match tree.kind(view) {
        Some(_) => kind_name(tree.kind_number(view))
            .trim_start_matches("Kind")
            .to_owned(),
        None => "List".to_owned(),
    };
    out.push_str(&format!(
        "{}{name} {} {}\n",
        " ".repeat(depth),
        pos as i32,
        end as i32
    ));
    let mut children = Vec::new();
    tree.children(view, &mut children);
    for child in children {
        dump(tree, child, depth + 1, out);
    }
}

/// A statement of a TSX file printed with tsgo's defaults (a new line,
/// four spaces).
#[track_caller]
fn assert_positions(text: &str, printed: &str, expected: &str) {
    let positioned = print_and_position(
        text,
        ScriptKind::Tsx,
        (NewLineKind::LineFeed, 4),
        LanguageVariant::Jsx,
    );
    assert_eq!(positioned.text, printed, "{text}");
    let tree = Tree::detached(&positioned.file);
    let mut out = String::new();
    dump(&tree, View::Node(positioned.node), 0, &mut out);
    assert_eq!(out, expected, "{text}");
}

#[track_caller]
fn assert_insertion(target: &str, position: usize, node: &str, settings: &str, expected: &str) {
    let target = parse_source_file(
        "/t.ts",
        target.to_owned(),
        ParseOptions {
            script_kind: ScriptKind::Ts,
            ..ParseOptions::default()
        },
        None,
    );
    let preferences = UserPreferences::from_json(&serde_json::from_str(settings).unwrap()).unwrap();
    let options = &preferences.format_code_settings;
    let new_line = if options.editor_settings.new_line_character == "\r\n" {
        NewLineKind::CarriageReturnLineFeed
    } else {
        NewLineKind::LineFeed
    };
    let indent = usize::try_from(options.editor_settings.indent_size).unwrap_or(4);
    let positioned = print_and_position(
        node,
        ScriptKind::Ts,
        (new_line, indent),
        target.language_variant,
    );
    // The position is UTF-16 in the API; the tests' targets are ASCII up to it
    // but for one, converted here.
    let utf16: Vec<u16> = target.text().encode_utf16().collect();
    let position = String::from_utf16(&utf16[..position]).unwrap().len();
    let formatted = tsc_ls::format_node_for_insertion(&target, position, &positioned, options);
    assert_eq!(formatted, expected, "{node}");
}

#[test]
fn positions_of_printed_statements() {
    assert_positions(
        "const x = 1;",
        "const x = 1;",
        "VariableStatement 0 12\n VariableDeclarationList 0 11\n  List 5 11\n   VariableDeclaration 5 11\n    Identifier 5 7\n    NumericLiteral 9 11\n",
    );
    assert_positions(
        "import { a, b as c } from \"m\";",
        "import { a, b as c } from \"m\";",
        "ImportDeclaration 0 30\n ImportClause 6 20\n  NamedImports 6 20\n   List 8 18\n    ImportSpecifier 8 10\n     Identifier 8 10\n    ImportSpecifier 11 18\n     Identifier 11 13\n     Identifier 16 18\n StringLiteral 25 29\n",
    );
    assert_positions(
        "function f<T, U extends T = T>(a: T, ...b: U[]): void { return; }",
        "function f<T, U extends T = T>(a: T, ...b: U[]): void { return; }",
        "FunctionDeclaration 0 65\n Identifier 8 10\n List 11 29\n  TypeParameter 11 12\n   Identifier 11 12\n  TypeParameter 13 29\n   Identifier 13 15\n   TypeReference 23 25\n    Identifier 23 25\n   TypeReference 27 29\n    Identifier 27 29\n List 31 46\n  Parameter 31 35\n   Identifier 31 32\n   TypeReference 33 35\n    Identifier 33 35\n  Parameter 36 46\n   DotDotDotToken 36 40\n   Identifier 40 41\n   ArrayType 42 46\n    TypeReference 42 44\n     Identifier 42 44\n VoidKeyword 48 53\n Block 53 65\n  List 55 63\n   ReturnStatement 55 63\n",
    );
    assert_positions(
        "class C<T> extends B<T> implements I, J { private readonly x: number = 1; static m(a, b?) { } get p() { return 1; } }",
        "class C<T> extends B<T> implements I, J {\n    private readonly x: number = 1;\n    static m(a, b?) {\n    }\n    get p() { return 1; }\n}",
        "ClassDeclaration 0 133\n Identifier 5 7\n List 8 9\n  TypeParameter 8 9\n   Identifier 8 9\n List 10 39\n  HeritageClause 10 23\n   List 18 23\n    ExpressionWithTypeArguments 18 23\n     Identifier 18 20\n     List 21 22\n      TypeReference 21 22\n       Identifier 21 22\n  HeritageClause 23 39\n   List 34 39\n    TypeReference 34 36\n     Identifier 34 36\n    TypeReference 37 39\n     Identifier 37 39\n List 41 131\n  PropertyDeclaration 41 77\n   List -1 -1\n    PrivateKeyword 41 53\n    ReadonlyKeyword 53 62\n   Identifier 62 64\n   NumberKeyword 65 72\n   NumericLiteral 74 76\n  MethodDeclaration 77 105\n   List -1 -1\n    StaticKeyword 77 88\n   Identifier 88 90\n   List 91 96\n    Parameter 91 92\n     Identifier 91 92\n    Parameter 93 96\n     Identifier 93 95\n     QuestionToken 95 96\n   Block 97 105\n    List 99 99\n  GetAccessor 105 131\n   Identifier 113 115\n   List 116 116\n   Block 117 131\n    List 119 129\n     ReturnStatement 119 129\n      NumericLiteral 126 128\n",
    );
    assert_positions(
        "let o = { a: 1, b: [1, 2, , 3], c() {}, ...d, [e]: f };",
        "let o = { a: 1, b: [1, 2, , 3], c() {\n    }, ...d, [e]: f };",
        "VariableStatement 0 60\n VariableDeclarationList 0 59\n  List 3 59\n   VariableDeclaration 3 59\n    Identifier 3 5\n    ObjectLiteralExpression 7 59\n     List 9 57\n      PropertyAssignment 9 14\n       Identifier 9 11\n       NumericLiteral 12 14\n      PropertyAssignment 15 30\n       Identifier 15 17\n       ArrayLiteralExpression 18 30\n        List 20 29\n         NumericLiteral 20 21\n         NumericLiteral 22 24\n         OmittedExpression 25 25\n         NumericLiteral 27 29\n      MethodDeclaration 31 43\n       Identifier 31 33\n       List 34 34\n       Block 35 43\n        List 37 37\n      SpreadAssignment 44 49\n       Identifier 48 49\n      PropertyAssignment 50 57\n       ComputedPropertyName 50 54\n        Identifier 52 53\n       Identifier 55 57\n",
    );
    assert_positions(
        "f(a, (b) => b + 1, async c => { await c; });",
        "f(a, b => b + 1, async (c) => { await c; });",
        "ExpressionStatement 0 44\n CallExpression 0 43\n  Identifier 0 1\n  List 2 42\n   Identifier 2 3\n   ArrowFunction 4 15\n    List 4 6\n     Parameter 4 6\n      Identifier 4 6\n    EqualsGreaterThanToken 6 9\n    BinaryExpression 9 15\n     Identifier 9 11\n     PlusToken 11 13\n     NumericLiteral 13 15\n   ArrowFunction 16 42\n    List -1 -1\n     AsyncKeyword 16 22\n    List 24 25\n     Parameter 24 25\n      Identifier 24 25\n    EqualsGreaterThanToken 26 29\n    Block 29 42\n     List 31 40\n      ExpressionStatement 31 40\n       AwaitExpression 31 39\n        Identifier 37 39\n",
    );
    assert_positions(
        "switch (x) { case 1: f(); break; default: }",
        "switch (x) {\n    case 1:\n        f();\n        break;\n    default:\n}",
        "SwitchStatement 0 67\n Identifier 8 9\n CaseBlock 10 67\n  List 12 65\n   CaseClause 12 52\n    NumericLiteral 21 23\n    List 24 52\n     ExpressionStatement 24 37\n      CallExpression 24 36\n       Identifier 24 34\n       List 35 35\n     BreakStatement 37 52\n   DefaultClause 52 65\n    List 65 65\n",
    );
    assert_positions(
        "try { a(); } catch (e) { b(e); } finally { c(); }",
        "try {\n    a();\n} catch (e) {\n    b(e);\n} finally {\n    c();\n}",
        "TryStatement 0 61\n Block 3 16\n  List 5 14\n   ExpressionStatement 5 14\n    CallExpression 5 13\n     Identifier 5 11\n     List 12 12\n CatchClause 16 40\n  VariableDeclaration 24 25\n   Identifier 24 25\n  Block 26 40\n   List 28 38\n    ExpressionStatement 28 38\n     CallExpression 28 37\n      Identifier 28 34\n      List 35 36\n       Identifier 35 36\n Block 48 61\n  List 50 59\n   ExpressionStatement 50 59\n    CallExpression 50 58\n     Identifier 50 56\n     List 57 57\n",
    );
    assert_positions(
        "@dec() class D { @prop() p: string; constructor(@inject private s: S) {} }",
        "@dec()\nclass D {\n    @prop()\n    p: string;\n    constructor(\n    @inject\n    private s: S) {\n    }\n}",
        "ClassDeclaration 0 100\n List -1 -1\n  Decorator 0 6\n   CallExpression 1 6\n    Identifier 1 4\n    List 5 5\n Identifier 12 14\n List 16 98\n  PropertyDeclaration 16 43\n   List -1 -1\n    Decorator 16 28\n     CallExpression 22 28\n      Identifier 22 26\n      List 27 27\n   Identifier 28 34\n   StringKeyword 35 42\n  Constructor 43 98\n   List 60 89\n    Parameter 60 89\n     List -1 -1\n      Decorator 60 72\n       Identifier 66 72\n      PrivateKeyword 72 84\n     Identifier 84 86\n     TypeReference 87 89\n      Identifier 87 89\n   Block 90 98\n    List 92 92\n",
    );
    assert_positions(
        "x = a ? b : c, y = !z, w = (v as T) satisfies U;",
        "x = a ? b : c, y = !z, w = (v as T) satisfies U;",
        "ExpressionStatement 0 48\n BinaryExpression 0 47\n  BinaryExpression 0 21\n   BinaryExpression 0 13\n    Identifier 0 1\n    EqualsToken 1 3\n    ConditionalExpression 3 13\n     Identifier 3 5\n     QuestionToken 5 7\n     Identifier 7 9\n     ColonToken 9 11\n     Identifier 11 13\n   CommaToken 13 14\n   BinaryExpression 14 21\n    Identifier 14 16\n    EqualsToken 16 18\n    PrefixUnaryExpression 18 21\n     Identifier 20 21\n  CommaToken 21 22\n  BinaryExpression 22 47\n   Identifier 22 24\n   EqualsToken 24 26\n   SatisfiesExpression 26 47\n    ParenthesizedExpression 26 35\n     AsExpression 28 34\n      Identifier 28 29\n      TypeReference 32 34\n       Identifier 32 34\n    TypeReference 45 47\n     Identifier 45 47\n",
    );
    assert_positions(
        "label: while (true) break label;",
        "label: while (true) break label;",
        "LabeledStatement 0 32\n Identifier 0 5\n WhileStatement 6 32\n  TrueKeyword 14 18\n  BreakStatement 19 32\n   Identifier 25 31\n",
    );
    assert_positions(
        "type M = { readonly [K in keyof T]?: T[K] };",
        "type M = {\n    readonly [K in keyof T]?: T[K];\n};",
        "TypeAliasDeclaration 0 49\n Identifier 4 6\n MappedType 8 48\n  ReadonlyKeyword 10 23\n  TypeParameter 25 37\n   Identifier 25 26\n   TypeOperator 29 37\n    TypeReference 35 37\n     Identifier 35 37\n  QuestionToken 38 39\n  IndexedAccessType 40 45\n   TypeReference 40 42\n    Identifier 40 42\n   TypeReference 43 44\n    Identifier 43 44\n  List 0 0\n",
    );
    assert_positions(
        "type U<T> = A | B & C<T> | { a: string; b?(): void } | [x: number, ...y: string[]];",
        "type U<T> = A | (B & C<T>) | {\n    a: string;\n    b?(): void;\n} | [\n    x: number,\n    ...y: string[]\n];",
        "TypeAliasDeclaration 0 104\n Identifier 4 6\n List 7 8\n  TypeParameter 7 8\n   Identifier 7 8\n UnionType 11 103\n  List 11 103\n   TypeReference 11 13\n    Identifier 11 13\n   IntersectionType 17 25\n    List 17 25\n     TypeReference 17 18\n      Identifier 17 18\n     TypeReference 20 25\n      Identifier 20 22\n      List 23 24\n       TypeReference 23 24\n        Identifier 23 24\n   TypeLiteral 28 63\n    List 30 61\n     PropertySignature 30 45\n      Identifier 30 36\n      StringKeyword 37 44\n     MethodSignature 45 61\n      Identifier 45 51\n      QuestionToken 51 52\n      List 53 53\n      VoidKeyword 55 60\n   TupleType 65 103\n    List 67 101\n     NamedTupleMember 67 81\n      Identifier 67 73\n      NumberKeyword 74 81\n     NamedTupleMember 82 101\n      DotDotDotToken 82 90\n      Identifier 90 91\n      ArrayType 92 101\n       StringKeyword 92 99\n",
    );
    assert_positions(
        "const j = <div a=\"b\" {...c}>text{d}</div>;",
        "const j = <div a=\"b\" {...c}>text{d}</div>;",
        "VariableStatement 0 42\n VariableDeclarationList 0 41\n  List 5 41\n   VariableDeclaration 5 41\n    Identifier 5 7\n    JsxElement 9 41\n     JsxOpeningElement 9 28\n      Identifier 11 14\n      JsxAttributes 14 27\n       List 14 27\n        JsxAttribute 14 20\n         Identifier 14 16\n         StringLiteral 17 20\n        JsxSpreadAttribute 20 27\n         Identifier 25 26\n     List 28 35\n      JsxText 28 32\n      JsxExpression 32 35\n       Identifier 33 34\n     JsxClosingElement 35 41\n      Identifier 37 40\n",
    );
    assert_positions(
        "let nn = a!.b!;",
        "let nn = (a!).b!;",
        "VariableStatement 0 17\n VariableDeclarationList 0 16\n  List 3 16\n   VariableDeclaration 3 16\n    Identifier 3 6\n    NonNullExpression 8 16\n     PropertyAccessExpression 8 15\n      NonNullExpression 10 12\n       Identifier 10 11\n      Identifier 14 15\n",
    );
    assert_positions(
        "const a = [1, 2,];",
        "const a = [1, 2];",
        "VariableStatement 0 17\n VariableDeclarationList 0 16\n  List 5 16\n   VariableDeclaration 5 16\n    Identifier 5 7\n    ArrayLiteralExpression 9 16\n     List 11 15\n      NumericLiteral 11 12\n      NumericLiteral 13 15\n",
    );
    assert_positions(
        "const h = <T,>(x: T): T => x;",
        "const h = <T>(x: T): T => x;",
        "VariableStatement 0 28\n VariableDeclarationList 0 27\n  List 5 27\n   VariableDeclaration 5 27\n    Identifier 5 7\n    ArrowFunction 9 27\n     List 11 12\n      TypeParameter 11 12\n       Identifier 11 12\n     List 14 18\n      Parameter 14 18\n       Identifier 14 15\n       TypeReference 16 18\n        Identifier 16 18\n     TypeReference 20 22\n      Identifier 20 22\n     EqualsGreaterThanToken 22 25\n     Identifier 25 27\n",
    );
    assert_positions(
        "let re = /ab+c/gi, big = 10n, s = 'single';",
        "let re = /ab+c/gi, big = 10n, s = 'single';",
        "VariableStatement 0 43\n VariableDeclarationList 0 42\n  List 3 42\n   VariableDeclaration 3 17\n    Identifier 3 6\n    RegularExpressionLiteral 8 17\n   VariableDeclaration 18 28\n    Identifier 18 22\n    BigIntLiteral 24 28\n   VariableDeclaration 29 42\n    Identifier 29 31\n    StringLiteral 33 42\n",
    );
    assert_positions(
        "enum E { A = 1, B }",
        "enum E {\n    A = 1,\n    B\n}",
        "EnumDeclaration 0 27\n Identifier 4 6\n List 8 25\n  EnumMember 8 18\n   Identifier 8 14\n   NumericLiteral 16 18\n  EnumMember 19 25\n   Identifier 19 25\n",
    );
    assert_positions(
        "declare module \"m\" { export const x: number; }",
        "declare module \"m\" {\n    export const x: number;\n}",
        "ModuleDeclaration 0 50\n List -1 -1\n  DeclareKeyword 0 7\n StringLiteral 14 18\n ModuleBlock 18 50\n  List 20 48\n   VariableStatement 20 48\n    List -1 -1\n     ExportKeyword 20 31\n    VariableDeclarationList 31 47\n     List 37 47\n      VariableDeclaration 37 47\n       Identifier 37 39\n       NumberKeyword 40 47\n",
    );
    // tsgo's AssignPositionsToNode lifts the missing `else` into an empty
    // block without a position (liftToBlock of a nil statement), which its
    // formatter then asserts against; tsc-rs keeps the tree as printed.
    assert_positions(
        "function f() { if (a) { b(); } }",
        "function f() { if (a) {\n    b();\n} }",
        "FunctionDeclaration 0 36\n Identifier 8 10\n List 11 11\n Block 12 36\n  List 14 34\n   IfStatement 14 34\n    Identifier 19 20\n    Block 21 34\n     List 23 32\n      ExpressionStatement 23 32\n       CallExpression 23 31\n        Identifier 23 29\n        List 30 30\n",
    );
    assert_positions(
        "const t = `a${b}c${d}`;",
        "const t = `a${b}c${d}`;",
        "VariableStatement 0 23\n VariableDeclarationList 0 22\n  List 5 22\n   VariableDeclaration 5 22\n    Identifier 5 7\n    TemplateExpression 9 22\n     TemplateHead 9 14\n     List 14 22\n      TemplateSpan 14 19\n       Identifier 14 15\n       TemplateMiddle 15 19\n      TemplateSpan 19 22\n       Identifier 19 20\n       TemplateTail 20 22\n",
    );
}

#[test]
fn formatted_for_insertion() {
    // tsgo fails an assertion on the `else` its positioning adds to an `if`
    // without one (see `positions_of_printed_statements`); tsc-rs formats
    // the statement as it formats one with an `else`.
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "if (a) { b(); }",
        "{}",
        "    if (a) {\n        b();\n    }",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "const x = 1;",
        "{}",
        "    const x = 1;",
    );
    assert_insertion(
        "const a = 1;\nconst b = 2;\n",
        0,
        "const y = 42;",
        "{}",
        "const y = 42;",
    );
    assert_insertion(
        "// é\nfunction greet(name: string) {\n    console.log(name);\n}\n",
        36,
        "const x = 1;",
        "{}",
        "    const x = 1;",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "function f(a, b) { return a + b; }",
        "{}",
        "    function f(a, b) { return a + b; }",
    );
    assert_insertion(
        "class A {\n    m() {\n        if (x) {\n            y();\n        }\n    }\n}\n",
        37,
        "function f(a, b) { return a + b; }",
        "{}",
        "            function f(a, b) { return a + b; }",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "class C { m() { return 1; } p = 2; }",
        "{}",
        "    class C {\n        m() { return 1; }\n        p = 2;\n    }",
    );
    assert_insertion(
        "class A {\n    m() {\n        if (x) {\n            y();\n        }\n    }\n}\n",
        37,
        "class C { m() { return 1; } p = 2; }",
        "{}",
        "            class C {\n                m() { return 1; }\n                p = 2;\n            }",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "if (a) { b(); } else { c(); }",
        "{}",
        "    if (a) {\n        b();\n    } else {\n        c();\n    }",
    );
    assert_insertion(
        "class A {\n    m() {\n        if (x) {\n            y();\n        }\n    }\n}\n",
        37,
        "if (a) { b(); } else { c(); }",
        "{}",
        "            if (a) {\n                b();\n            } else {\n                c();\n            }",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "switch (k) { case 1: a(); break; default: b(); }",
        "{}",
        "    switch (k) {\n        case 1:\n            a();\n            break;\n        default: b();\n    }",
    );
    assert_insertion(
        "class A {\n    m() {\n        if (x) {\n            y();\n        }\n    }\n}\n",
        37,
        "switch (k) { case 1: a(); break; default: b(); }",
        "{}",
        "            switch (k) {\n                case 1:\n                    a();\n                    break;\n                default: b();\n            }",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "try { a(); } catch (e) { b(e); } finally { c(); }",
        "{}",
        "    try {\n        a();\n    } catch (e) {\n        b(e);\n    } finally {\n        c();\n    }",
    );
    assert_insertion(
        "class A {\n    m() {\n        if (x) {\n            y();\n        }\n    }\n}\n",
        37,
        "try { a(); } catch (e) { b(e); } finally { c(); }",
        "{}",
        "            try {\n                a();\n            } catch (e) {\n                b(e);\n            } finally {\n                c();\n            }",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "const o = { a: 1, b: { c: [1, 2] }, m() { return this.a; } };",
        "{}",
        "    const o = { a: 1, b: { c: [1, 2] }, m() { return this.a; } };",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "interface I { a: string; m(x: number): void; }",
        "{}",
        "    interface I {\n        a: string;\n        m(x: number): void;\n    }",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "type T = { a: string; b: { c: number } };",
        "{}",
        "    type T = {\n        a: string;\n        b: {\n            c: number;\n        };\n    };",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "enum E { A = 1, B, C }",
        "{}",
        "    enum E {\n        A = 1,\n        B,\n        C\n    }",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "import { a, b } from \"m\";",
        "{}",
        "    import { a, b } from \"m\";",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "export const z = () => { return 1; };",
        "{}",
        "    export const z = () => { return 1; };",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "namespace N { export const x = 1; }",
        "{}",
        "    namespace N {\n        export const x = 1;\n    }",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "while (a) { do { b(); } while (c); }",
        "{}",
        "    while (a) {\n        do {\n            b();\n        } while (c);\n    }",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "f(a, function () { return 1; }, () => { x(); });",
        "{}",
        "    f(a, function() { return 1; }, () => { x(); });",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "const j = [1, [2, 3], { a: 4 }];",
        "{}",
        "    const j = [1, [2, 3], { a: 4 }];",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "let s = `a${b}c`;",
        "{}",
        "    let s = `a${b}c`;",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "label: for (;;) { break label; }",
        "{}",
        "    label: for (; ;) {\n        break label;\n    }",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "if (a) b(); else c();",
        "{}",
        "    if (a) b(); else c();",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "abstract class K<T> extends B<T> implements I { abstract m(): void; constructor(private x: T) { super(); } }",
        "{}",
        "    abstract class K<T> extends B<T> implements I {\n        abstract m(): void;\n        constructor(private x: T) { super(); }\n    }",
    );
    assert_insertion(
        "class A {\n    m() {\n        if (x) {\n            y();\n        }\n    }\n}\n",
        37,
        "abstract class K<T> extends B<T> implements I { abstract m(): void; constructor(private x: T) { super(); } }",
        "{}",
        "            abstract class K<T> extends B<T> implements I {\n                abstract m(): void;\n                constructor(private x: T) { super(); }\n            }",
    );
    assert_insertion(
        "",
        0,
        "abstract class K<T> extends B<T> implements I { abstract m(): void; constructor(private x: T) { super(); } }",
        "{}",
        "abstract class K<T> extends B<T> implements I {\n    abstract m(): void;\n    constructor(private x: T) { super(); }\n}",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "x = a ? b : c;",
        "{}",
        "    x = a ? b : c;",
    );
    assert_insertion(
        "class A {\n    m() {\n        if (x) {\n            y();\n        }\n    }\n}\n",
        37,
        "x = a ? b : c;",
        "{}",
        "            x = a ? b : c;",
    );
    assert_insertion(
        "class A {\n    m() {\n        if (x) {\n            y();\n        }\n    }\n}\n",
        49,
        "const q = 1;",
        "{}",
        "            const q = 1;",
    );
    assert_insertion(
        "class A {\n    m() {\n        if (x) {\n            y();\n        }\n    }\n}\n",
        14,
        "n() { return 2; }",
        "{}",
        "    n();",
    );
    assert_insertion(
        "class A {\n    m() {\n        if (x) {\n            y();\n        }\n    }\n}\n",
        72,
        "class D { }",
        "{}",
        "class D {\n}",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        30,
        "const after = { a: 1 };",
        "{}",
        "const after = { a: 1 };",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "function f() { return { a: 1 }; }",
        "{\"placeOpenBraceOnNewLineForFunctions\": true}",
        "    function f() { return { a: 1 }; }",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "const o = { a: 1 };",
        "{\"insertSpaceAfterOpeningAndBeforeClosingNonemptyBraces\": false}",
        "    const o = {a: 1};",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "const o = { a: 1 };",
        "{\"newLineCharacter\": \"\\r\\n\"}",
        "    const o = { a: 1 };",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "function f() { a(); }",
        "{\"newLineCharacter\": \"\\r\\n\"}",
        "    function f() { a(); }",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "function f() { a(); }",
        "{\"indentSize\": 0}",
        "function f() { a(); }",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "function f() { a(); }",
        "{\"baseIndentSize\": 3}",
        "    function f() { a(); }",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "function f() { a(); }",
        "{\"indentStyle\": \"none\"}",
        "function f() { a(); }",
    );
    assert_insertion(
        "function greet(name: string) {\n    console.log(name);\n}\n",
        31,
        "function f() { a(); }",
        "{\"semicolons\": \"remove\"}",
        "    function f() { a() }",
    );
}

/// The indentation of a statement inserted at each position of a file
/// (`GetIndentation` through `formatNodeForInsertion`): comments, string
/// and template literals, list items after a comma or before the first,
/// object literals, the smart indenter's parents, the block and no indent
/// styles. One width per position, from tsgo.
#[test]
fn indentation_of_an_insertion_at_each_position() {
    assert_indentation_at_each_position(
        "function greet(name: string) {\n    console.log(name);\n    if (name) {\n        return [\n            1,\n            2,\n        ];\n    }\n}\n",
        "{}",
        &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 0, 0, 0, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 8, 8, 8, 8, 8, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 8, 8, 8, 8, 8, 8, 4, 4, 0, 0],
    );
    assert_indentation_at_each_position(
        "/* block\n * comment\n */\nconst o = {\n    a: 1,\n    b: foo(x,\n        y),\n    c: `t ${x}\n more`,\n    \"s\": 'q',\n};\n// line comment\nswitch (k) {\n    case 1:\n        f();\n    default:\n}\n",
        "{}",
        &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 4, 0, 0, 0, 0, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 0, 0, 0, 0, 4, 4, 0, 0, 0, 0, 0, 0, 0, 4, 4, 4, 4, 4, 4, 4, 0, 0, 4, 4, 4, 0, 0, 4, 4, 4, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 12, 8, 8, 8, 8, 8, 8, 8, 4, 4, 4, 4, 4, 4, 4, 4, 8, 0, 0],
    );
    assert_indentation_at_each_position(
        "class C<T> extends B {\n    private x = 1;\n    m(a: number,\n      b: string): void {\n        const f = (v) =>\n            v + 1;\n        call(a, () => {\n            other();\n        });\n    }\n}\ninterface I {\n    a: string;\n}\n",
        "{}",
        &[0, 0, 0, 0, 0, 0, 0, 0, 4, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 8, 4, 4, 4, 4, 4, 4, 4, 4, 8, 8, 8, 8, 8, 8, 8, 8, 8, 6, 6, 6, 6, 6, 6, 6, 6, 6, 8, 8, 8, 4, 4, 4, 4, 4, 4, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 12, 12, 8, 8, 8, 8, 12, 12, 8, 8, 8, 8, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 12, 12, 12, 12, 8, 8, 8, 8, 8, 8, 8, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 16, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 8, 12, 8, 8, 8, 8, 8, 8, 4, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 0, 0],
    );
    assert_indentation_at_each_position(
        "namespace N {\n    export enum E {\n        A,\n        B,\n    }\n}\nlet x = cond\n    ? a\n    : b;\nfor (let i = 0;\n    i < n; i++) {\n}\ntype T = {\n    k: number\n};\n",
        "{}",
        &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 4, 4, 4, 4, 4, 4, 0, 0, 0, 0, 4, 4, 0, 0, 0, 0, 0, 0, 0, 0, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 4, 0, 0, 0, 0, 0, 0, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 0, 0],
    );
    assert_indentation_at_each_position(
        "function greet(name: string) {\n    console.log(name);\n    if (name) {\n        return [\n            1,\n            2,\n        ];\n    }\n}\n",
        "{\"indentStyle\": \"block\"}",
        &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 8, 8, 8, 8, 8, 8, 8, 4, 4, 0, 0, 0],
    );
    assert_indentation_at_each_position(
        "function greet(name: string) {\n    console.log(name);\n    if (name) {\n        return [\n            1,\n            2,\n        ];\n    }\n}\n",
        "{\"indentStyle\": \"none\"}",
        &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    );
}

#[track_caller]
fn assert_indentation_at_each_position(target: &str, settings: &str, expected: &[usize]) {
    let source = parse_source_file(
        "/t.ts",
        target.to_owned(),
        ParseOptions {
            script_kind: ScriptKind::Ts,
            ..ParseOptions::default()
        },
        None,
    );
    let preferences = UserPreferences::from_json(&serde_json::from_str(settings).unwrap()).unwrap();
    let options = &preferences.format_code_settings;
    let positioned = print_and_position(
        "x;",
        ScriptKind::Ts,
        (NewLineKind::LineFeed, 4),
        source.language_variant,
    );
    let widths: Vec<usize> = (0..=target.len())
        .map(|position| {
            let formatted =
                tsc_ls::format_node_for_insertion(&source, position, &positioned, options);
            formatted.len() - formatted.trim_start_matches(' ').len()
        })
        .collect();
    assert_eq!(widths, expected, "{target}");
}
