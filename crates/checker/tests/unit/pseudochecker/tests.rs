use tsc_syntax::{NodeData, NodeId, SyntaxKind};
use tsc_types::CompilerOptions;

use crate::state::test_support::with_program_state;
use crate::state::CheckerState;

use super::*;

fn text(checker: &CheckerState<'_>, node: NodeId) -> String {
    checker.text_of_node(node).expect("node text")
}

/// A compact spelling of a pseudo type, with the source text of its nodes.
fn render(checker: &CheckerState<'_>, pseudo: &PseudoType) -> String {
    match pseudo {
        PseudoType::Direct(node) => format!("Direct({})", text(checker, *node)),
        PseudoType::Inferred(inferred) => {
            let mut result = format!("Inferred({}", text(checker, inferred.expression));
            if !inferred.error_nodes.is_empty() {
                let errors: Vec<_> = inferred
                    .error_nodes
                    .iter()
                    .map(|&node| text(checker, node))
                    .collect();
                result.push_str(&format!(" errors=[{}]", errors.join(", ")));
            }
            if inferred.is_signature_return {
                result.push_str(" signature");
            }
            result + ")"
        }
        PseudoType::NoResult(node) => format!("NoResult({:?})", checker.kind_of(*node)),
        PseudoType::MaybeConstLocation(location) => format!(
            "Maybe({} | {})",
            render(checker, &location.const_type),
            render(checker, &location.regular_type)
        ),
        PseudoType::Union(members) => {
            let members: Vec<_> = members
                .iter()
                .map(|member| render(checker, member))
                .collect();
            format!("Union({})", members.join(", "))
        }
        PseudoType::Undefined => "undefined".to_owned(),
        PseudoType::Null => "null".to_owned(),
        PseudoType::String => "string".to_owned(),
        PseudoType::Number => "number".to_owned(),
        PseudoType::BigInt => "bigint".to_owned(),
        PseudoType::Boolean => "boolean".to_owned(),
        PseudoType::False => "false".to_owned(),
        PseudoType::True => "true".to_owned(),
        PseudoType::SingleCallSignature(signature) => {
            let type_parameters: Vec<_> = signature
                .type_parameters
                .iter()
                .map(|&node| text(checker, node))
                .collect();
            let parameters: Vec<_> = signature
                .parameters
                .iter()
                .map(|parameter| render_parameter(checker, parameter))
                .collect();
            format!(
                "<{}>({}) => {}",
                type_parameters.join(", "),
                parameters.join(", "),
                render(checker, &signature.return_type)
            )
        }
        PseudoType::Tuple(elements) => {
            let elements: Vec<_> = elements
                .iter()
                .map(|element| render(checker, element))
                .collect();
            format!("[{}]", elements.join(", "))
        }
        PseudoType::ObjectLiteral(elements) => {
            let elements: Vec<_> = elements
                .iter()
                .map(|element| render_element(checker, element))
                .collect();
            format!("{{{}}}", elements.join("; "))
        }
        PseudoType::StringLiteral(node)
        | PseudoType::NumericLiteral(node)
        | PseudoType::BigIntLiteral(node) => format!("Literal({})", text(checker, *node)),
    }
}

fn render_parameter(checker: &CheckerState<'_>, parameter: &PseudoParameter) -> String {
    let name = match checker.data_of(parameter.parameter) {
        NodeData::Parameter(data) => text(checker, data.name.expect("parameter name")),
        _ => text(checker, parameter.parameter),
    };
    format!(
        "{}{}{}: {}",
        if parameter.rest { "..." } else { "" },
        name,
        if parameter.optional { "?" } else { "" },
        render(checker, &parameter.r#type)
    )
}

fn render_element(checker: &CheckerState<'_>, element: &PseudoObjectElement) -> String {
    let name = text(checker, element.name);
    let optional = if element.optional { "?" } else { "" };
    match &element.kind {
        PseudoObjectElementKind::Method {
            parameters,
            return_type,
            ..
        } => {
            let parameters: Vec<_> = parameters
                .iter()
                .map(|parameter| render_parameter(checker, parameter))
                .collect();
            format!(
                "{name}{optional}({}): {}",
                parameters.join(", "),
                render(checker, return_type)
            )
        }
        PseudoObjectElementKind::PropertyAssignment { readonly, r#type } => format!(
            "{}{name}{optional}: {}",
            if *readonly { "readonly " } else { "" },
            render(checker, r#type)
        ),
        PseudoObjectElementKind::GetAccessor { r#type, .. } => {
            format!("get {name}: {}", render(checker, r#type))
        }
        PseudoObjectElementKind::SetAccessor { parameter, .. } => {
            format!("set {name}({})", render_parameter(checker, parameter))
        }
    }
}

fn statements(checker: &CheckerState<'_>) -> Vec<NodeId> {
    let root = checker.binder.source(0).root;
    match checker.data_of(root) {
        NodeData::SourceFile(data) => checker.nodes_of(data.statements),
        _ => Vec::new(),
    }
}

fn variable(checker: &CheckerState<'_>, name: &str) -> NodeId {
    for statement in statements(checker) {
        let NodeData::VariableStatement(data) = checker.data_of(statement) else {
            continue;
        };
        let Some(NodeData::VariableDeclarationList(list)) =
            data.declaration_list.map(|list| checker.data_of(list))
        else {
            continue;
        };
        for declaration in checker.nodes_of(list.declarations) {
            if let NodeData::VariableDeclaration(data) = checker.data_of(declaration) {
                if data.name.and_then(|name| checker.identifier_text_of(name)) == Some(name) {
                    return declaration;
                }
            }
        }
    }
    panic!("variable {name}")
}

fn function(checker: &CheckerState<'_>, name: &str) -> NodeId {
    statements(checker)
        .into_iter()
        .find(|&statement| {
            matches!(
                checker.data_of(statement),
                NodeData::FunctionDeclaration(data)
                    if data.name.and_then(|name| checker.identifier_text_of(name)) == Some(name)
            )
        })
        .unwrap_or_else(|| panic!("function {name}"))
}

fn class_members(checker: &CheckerState<'_>) -> Vec<NodeId> {
    statements(checker)
        .into_iter()
        .find_map(|statement| match checker.data_of(statement) {
            NodeData::ClassDeclaration(data) => Some(checker.nodes_of(data.members)),
            _ => None,
        })
        .expect("class")
}

fn strict() -> CompilerOptions {
    CompilerOptions {
        strict: Some(true),
        ..CompilerOptions::default()
    }
}

fn declaration_types(
    file: &str,
    source: &str,
    options: &CompilerOptions,
    names: &[&str],
) -> Vec<String> {
    with_program_state(&[(file, source)], options, |checker| {
        let pseudochecker = PseudoChecker::new(checker);
        names
            .iter()
            .map(|name| {
                let declaration = variable(checker, name);
                format!(
                    "{name}: {}",
                    render(checker, &pseudochecker.get_type_of_declaration(declaration))
                )
            })
            .collect()
    })
}

/// lookup.go typeFromVariable and typeFromExpression: literals depend on the
/// const location, assertions read their type, a template with holes in a
/// `const` and a call give no result, and an array outside a const context
/// is its own error.
#[test]
fn variable_initializers_follow_type_from_expression() {
    let source = r#"
        declare function call(): number;
        const a = 1;
        let b = 'x';
        const c = -1;
        const d = +1;
        const e = 10n;
        const f = true;
        const g = null;
        const h = undefined;
        const i = `t`;
        const j = `t${a}`;
        let k = `t${a}`;
        const l = call();
        const m: number = 1;
        const n = [1, 'a'] as const;
        const o = [1, 2];
        const p = { x: 1 } as const;
        const q = <const>[false];
        const r = (1 as number);
        const s = [...o] as const;
    "#;
    assert_eq!(
        declaration_types(
            "/main.ts",
            source,
            &strict(),
            &[
                "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p",
                "q", "r", "s"
            ]
        ),
        [
            "a: Maybe(Literal(1) | number)",
            "b: Maybe(Literal('x') | string)",
            "c: Maybe(Literal(-1) | number)",
            "d: Maybe(Literal(1) | number)",
            "e: Maybe(Literal(10n) | bigint)",
            "f: Maybe(true | boolean)",
            "g: null",
            "h: undefined",
            "i: Maybe(Literal(`t`) | string)",
            "j: NoResult(VariableDeclaration)",
            "k: Maybe(Inferred(`t${a}`) | string)",
            "l: NoResult(VariableDeclaration)",
            "m: Direct(number)",
            "n: [Maybe(Literal(1) | number), Maybe(Literal('a') | string)]",
            "o: Inferred([1, 2] errors=[[1, 2]])",
            "p: {x: Maybe(Literal(1) | number)}",
            "q: [Maybe(false | boolean)]",
            "r: Direct(number)",
            "s: Inferred([...o] errors=[...o])",
        ]
    );
}

/// lookup.go typeFromObjectLiteral, getAccessorMember and
/// canGetTypeFromObjectLiteral: a method keeps its signature, an accessor
/// pair typed on both sides stays a pair, an accessor typed on one side is a
/// property (readonly for a lone getter), and shorthand, spread and
/// non-literal computed members are error nodes.
#[test]
fn object_literals_follow_type_from_object_literal() {
    let source = r#"
        declare const key: string;
        const shared = 1;
        const members = {
            m(x: number) { return x; },
            get both(): number { return 1; },
            set both(value: number) {},
            get lone(): string { return ''; },
            set first(value: boolean) {},
            get first() { return true; },
            'quoted': 1,
            [`literal`]: 2,
        };
        const errors = { shared, ...members, [key]: 3, ok: 4 };
        const empty = {};
    "#;
    assert_eq!(
        declaration_types(
            "/main.ts",
            source,
            &strict(),
            &["members", "errors", "empty"]
        ),
        [
            "members: {m(x: Direct(number)): Inferred(x); get both: Direct(number); \
             set both(value: Direct(number)); readonly lone: Direct(string); \
             first: Direct(boolean); 'quoted': Maybe(Literal(1) | number); \
             [`literal`]: Maybe(Literal(2) | number)}",
            "errors: Inferred({ shared, ...members, [key]: 3, ok: 4 } \
             errors=[shared, ...members, [key]])",
            "empty: {}",
        ]
    );
}

/// lookup.go createReturnFromSignature and typeFromSingleReturnExpression: a
/// written return type, the single top-level return expression, or the
/// signature itself.
#[test]
fn return_types_follow_create_return_from_signature() {
    let source = r#"
        declare const x: boolean;
        function annotated(): number { return 1; }
        function single() { return 'a'; }
        function nested() { if (x) { return 1; } }
        function twice() { if (x) return 1; return 2; }
        function none() {}
        async function asynchronous() { return 1; }
        function* generator() { return 1; }
        function asserted() { return 1 as number; }
    "#;
    let names = [
        "annotated",
        "single",
        "nested",
        "twice",
        "none",
        "asynchronous",
        "generator",
        "asserted",
    ];
    let types = with_program_state(&[("/main.ts", source)], &strict(), |checker| {
        let pseudochecker = PseudoChecker::new(checker);
        names
            .iter()
            .map(|name| {
                let declaration = function(checker, name);
                match pseudochecker.get_return_type_of_signature(declaration) {
                    PseudoType::Inferred(inferred) => format!(
                        "{name}: Inferred({:?}{})",
                        checker.kind_of(inferred.expression),
                        if inferred.is_signature_return {
                            " signature"
                        } else {
                            ""
                        }
                    ),
                    other => format!("{name}: {}", render(checker, &other)),
                }
            })
            .collect::<Vec<_>>()
    });
    assert_eq!(
        types,
        [
            "annotated: Direct(number)",
            "single: Maybe(Literal('a') | string)",
            "nested: Inferred(FunctionDeclaration signature)",
            "twice: Inferred(FunctionDeclaration signature)",
            "none: Inferred(FunctionDeclaration signature)",
            "asynchronous: Inferred(FunctionDeclaration signature)",
            "generator: Inferred(FunctionDeclaration signature)",
            "asserted: Direct(number)",
        ]
    );
}

/// lookup.go typeFromFunctionLikeExpression, cloneParameters and
/// typeFromParameterWorker: an initialized parameter is optional only when
/// no required parameter follows, and before a required one its type takes
/// `| undefined` under strictNullChecks. A rest parameter is not optional.
#[test]
fn function_expressions_clone_parameters_like_the_checker() {
    let source = r#"
        const f = <T>(a = 1, b: string, c?: number, d = 'x', ...rest: T[]) => b;
        const g = function (this: void, n: number = 1, m: boolean) { return n; };
    "#;
    assert_eq!(
        declaration_types("/main.ts", source, &strict(), &["f", "g"]),
        [
            "f: <T>(a: Union(Maybe(Literal(1) | number), undefined), b: Direct(string), \
             c?: Direct(number), d?: Maybe(Literal('x') | string), ...rest: Direct(T[])) \
             => Inferred(b)",
            "g: <>(this: Direct(void), n: Union(Direct(number), undefined), \
             m: Direct(boolean)) => Inferred(n)",
        ]
    );
}

/// lookup.go typeFromProperty, typeFromAccessor and the set-accessor
/// parameter: a class property's initializer, `?` adding `| undefined`, a
/// readonly template giving no result, and an accessor reading the other
/// accessor's annotation or the getter's return.
#[test]
fn class_members_follow_type_from_property_and_accessor() {
    let source = r#"
        class C {
            p = 1;
            q? = 'x';
            readonly r = `t${1}`;
            s;
            get t() { return 1; }
            set t(value) {}
            get u(): number { return 1; }
            set u(value) {}
            set v(value: string) {}
        }
    "#;
    let types = with_program_state(&[("/main.ts", source)], &strict(), |checker| {
        let pseudochecker = PseudoChecker::new(checker);
        class_members(checker)
            .into_iter()
            .map(|member| {
                let pseudo = match checker.kind_of(member) {
                    SyntaxKind::GetAccessor => pseudochecker.get_type_of_accessor(member),
                    SyntaxKind::SetAccessor => {
                        let parameter = checker.parameters_of_function(member)[0];
                        pseudochecker.get_type_of_declaration(parameter)
                    }
                    _ => pseudochecker.get_type_of_declaration(member),
                };
                format!(
                    "{:?}: {}",
                    checker.kind_of(member),
                    render(checker, &pseudo)
                )
            })
            .collect::<Vec<_>>()
    });
    assert_eq!(
        types,
        [
            "PropertyDeclaration: Maybe(Literal(1) | number)",
            "PropertyDeclaration: Union(Maybe(Literal('x') | string), undefined)",
            "PropertyDeclaration: NoResult(PropertyDeclaration)",
            "PropertyDeclaration: NoResult(PropertyDeclaration)",
            "GetAccessor: Maybe(Literal(1) | number)",
            "SetAccessor: Maybe(Literal(1) | number)",
            "GetAccessor: Direct(number)",
            "SetAccessor: Direct(number)",
            "SetAccessor: Direct(string)",
        ]
    );
}

/// lookup.go IsInConstContext: const-ness reaches through array and object
/// literals and parentheses up to the first assertion; a call does not
/// carry it.
#[test]
fn const_context_stops_at_the_first_assertion() {
    let source = r#"
        const a = [{ x: [(1)] }] as const;
        const b = [2 as number] as const;
        const c = f([3]);
        declare function f(value: unknown): unknown;
    "#;
    let found = with_program_state(&[("/main.ts", source)], &strict(), |checker| {
        let pseudochecker = PseudoChecker::new(checker);
        let mut literals = Vec::new();
        let mut stack = statements(checker);
        while let Some(node) = stack.pop() {
            if checker.kind_of(node) == SyntaxKind::NumericLiteral {
                literals.push((text(checker, node), pseudochecker.is_in_const_context(node)));
            }
            let source = checker.binder.source_of_node(node);
            tsc_syntax::for_each_child(&source.arena, source.arena.node(node), |child| {
                stack.push(child);
                false
            });
        }
        literals.sort();
        literals
    });
    assert_eq!(
        found,
        [
            ("1".to_owned(), true),
            ("2".to_owned(), false),
            ("3".to_owned(), false)
        ]
    );
}

/// lookup.go CouldAlreadyReferToUndefinedType: a reference, or a union with
/// one, might be `undefined`; keywords other than `undefined` are not.
#[test]
fn undefined_reference_check_reads_type_nodes() {
    let source = r#"
        type T = number;
        const a: T = 1;
        const b: string | T = 1;
        const c: string = '';
        const d: undefined = undefined;
        const e: (string) = '';
    "#;
    let found = with_program_state(&[("/main.ts", source)], &strict(), |checker| {
        let pseudochecker = PseudoChecker::new(checker);
        ["a", "b", "c", "d", "e"]
            .iter()
            .map(|name| {
                let pseudo = pseudochecker.get_type_of_declaration(variable(checker, name));
                could_already_refer_to_undefined_type(checker, &pseudo)
            })
            .collect::<Vec<_>>()
    });
    assert_eq!(found, [true, true, false, true, false]);
}

/// A JavaScript declaration's types are the ones tsgo's reparser hosts from
/// its JSDoc: a `@type` tag types the declaration, a `@type {const}` cast is
/// a const context, and a `@satisfies` cast is not read.
#[test]
fn javascript_reads_the_hosted_jsdoc_types() {
    let source = r#"
        /** @type {number} */
        const a = 1;
        const b = /** @type {const} */ ([1, 'x']);
        /** @satisfies {{ x: number }} */
        const c = { x: 1 };
        const d = /** @type {string} */ ('x');
    "#;
    let options = CompilerOptions {
        allow_js: true,
        check_js: Some(true),
        ..strict()
    };
    assert_eq!(
        declaration_types("/main.js", source, &options, &["a", "b", "c", "d"]),
        [
            "a: Direct(number)",
            "b: [Maybe(Literal(1) | number), Maybe(Literal('x') | string)]",
            "c: NoResult(VariableDeclaration)",
            "d: Direct(string)",
        ]
    );
}
