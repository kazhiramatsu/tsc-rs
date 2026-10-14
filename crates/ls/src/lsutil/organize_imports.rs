//! tsgo `ls/lsutil/organizeimports.go`: how organize imports (and the
//! import fixes) compare module specifiers and import or export specifiers,
//! and detect the order a file already uses. The functions read a node
//! arena, a parsed file's or the change tracker's copy of it.

use std::cmp::Ordering;

use tsc_syntax::{NodeArena, NodeData, NodeId, SyntaxKind};

use super::{
    OrganizeImportsCaseFirst, OrganizeImportsCollation, OrganizeImportsSort,
    OrganizeImportsTypeOrder, UserPreferences,
};
use tsc_syntax::go_unicode::tables::{CANONICAL_DECOMPOSITION, NONSPACING_MARK};
use tsc_syntax::go_unicode::{in_ranges, is_upper, to_lower as go_to_lower};

/// One of tsgo's organize imports string comparers (tsgo passes the
/// function).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StringComparer {
    /// `CompareStringsCaseSensitive`, or with `ignore_case`
    /// `CompareStringsCaseInsensitiveEslintCompatible`.
    Ordinal { ignore_case: bool },
    /// `compareOrganizeImportsNaturalStrings`.
    Natural { case_sensitive: bool },
    /// `compareOrganizeImportsUnicodeStrings`.
    Unicode {
        ignore_case: bool,
        case_first: OrganizeImportsCaseFirst,
        numeric: bool,
        accents: bool,
    },
}

impl StringComparer {
    pub(crate) fn compare(self, a: &str, b: &str) -> Ordering {
        match self {
            Self::Ordinal { ignore_case: false } => a.cmp(b),
            Self::Ordinal { ignore_case: true } => {
                if a == b {
                    return Ordering::Equal;
                }
                go_to_lower(a).cmp(&go_to_lower(b))
            }
            Self::Natural { case_sensitive } => {
                compare_strings_numeric(&natural_collation_key(a), &natural_collation_key(b))
                    .then_with(|| {
                        if case_sensitive {
                            compare_case(a, b, OrganizeImportsCaseFirst::UPPER)
                        } else {
                            Ordering::Equal
                        }
                    })
                    .then_with(|| a.cmp(b))
            }
            Self::Unicode {
                ignore_case,
                case_first,
                numeric,
                accents,
            } => compare_unicode_keys(
                &natural_collation_key(a),
                &natural_collation_key(b),
                numeric,
            )
            .then_with(|| {
                if accents {
                    compare_unicode_keys(&go_to_lower(a), &go_to_lower(b), numeric)
                } else {
                    Ordering::Equal
                }
            })
            .then_with(|| {
                if ignore_case {
                    Ordering::Equal
                } else {
                    compare_case(a, b, case_first)
                }
            })
            .then_with(|| a.cmp(b)),
        }
    }
}

/// tsgo `GetDetectionLists`: the comparers and type orders detection
/// tries.
pub(crate) fn get_detection_lists(
    preferences: &UserPreferences,
) -> (Vec<StringComparer>, Vec<OrganizeImportsTypeOrder>) {
    let comparers = if preferences.organize_imports_sort != OrganizeImportsSort::AUTO {
        vec![get_preset_string_comparer(
            preferences.organize_imports_sort,
        )]
    } else if let Some(ignore_case) = preferences.organize_imports_ignore_case {
        vec![get_string_comparer(preferences, ignore_case)]
    } else {
        vec![
            get_string_comparer(preferences, true),
            get_string_comparer(preferences, false),
        ]
    };
    let type_orders = if preferences.organize_imports_type_order != OrganizeImportsTypeOrder::AUTO {
        vec![preferences.organize_imports_type_order]
    } else {
        vec![
            OrganizeImportsTypeOrder::LAST,
            OrganizeImportsTypeOrder::INLINE,
            OrganizeImportsTypeOrder::FIRST,
        ]
    };
    (comparers, type_orders)
}

/// tsgo `ResolveOrganizeImportsSort`.
pub(crate) fn resolve_organize_imports_sort(preferences: &UserPreferences) -> OrganizeImportsSort {
    if preferences.organize_imports_sort != OrganizeImportsSort::AUTO {
        return preferences.organize_imports_sort;
    }
    let unicode = preferences.organize_imports_collation == OrganizeImportsCollation::UNICODE;
    match (unicode, preferences.organize_imports_ignore_case) {
        (true, Some(true)) => OrganizeImportsSort::NATURAL_IGNORE_CASE,
        (true, Some(false)) => OrganizeImportsSort::NATURAL,
        (false, Some(true)) => OrganizeImportsSort::ORDINAL_IGNORE_CASE,
        (false, Some(false)) => OrganizeImportsSort::ORDINAL,
        (_, None) => OrganizeImportsSort::AUTO,
    }
}

/// tsgo `getOrganizeImportsPresetStringComparer`.
fn get_preset_string_comparer(sort: OrganizeImportsSort) -> StringComparer {
    match sort {
        OrganizeImportsSort::ORDINAL_IGNORE_CASE => StringComparer::Ordinal { ignore_case: true },
        OrganizeImportsSort::NATURAL => StringComparer::Natural {
            case_sensitive: true,
        },
        OrganizeImportsSort::NATURAL_IGNORE_CASE => StringComparer::Natural {
            case_sensitive: false,
        },
        _ => StringComparer::Ordinal { ignore_case: false },
    }
}

/// tsgo `getOrganizeImportsStringComparer`.
pub(crate) fn get_string_comparer(
    preferences: &UserPreferences,
    ignore_case: bool,
) -> StringComparer {
    if preferences.organize_imports_sort != OrganizeImportsSort::AUTO {
        return get_preset_string_comparer(preferences.organize_imports_sort);
    }
    if preferences.organize_imports_collation == OrganizeImportsCollation::UNICODE {
        return StringComparer::Unicode {
            ignore_case,
            case_first: preferences.organize_imports_case_first,
            numeric: preferences.organize_imports_numeric_collation == Some(true),
            accents: preferences.organize_imports_accent_collation != Some(false),
        };
    }
    StringComparer::Ordinal { ignore_case }
}

/// tsgo `naturalCollationKey`: lower case without diacritics.
fn natural_collation_key(text: &str) -> String {
    go_to_lower(&remove_diacritics(text))
}

/// tsgo `removeDiacritics`: the canonical decomposition without its
/// nonspacing marks.
fn remove_diacritics(text: &str) -> String {
    if text.is_ascii() {
        return text.to_owned();
    }
    let mut result = String::with_capacity(text.len());
    for ch in text.chars() {
        let code = ch as u32;
        if (0xAC00..=0xD7A3).contains(&code) {
            // Hangul syllables decompose into Jamo, which are no marks.
            let index = code - 0xAC00;
            let (lead, vowel, trail) = (index / (21 * 28), index % (21 * 28) / 28, index % 28);
            result.extend(char::from_u32(0x1100 + lead));
            result.extend(char::from_u32(0x1161 + vowel));
            if trail != 0 {
                result.extend(char::from_u32(0x11A7 + trail));
            }
            continue;
        }
        match CANONICAL_DECOMPOSITION.binary_search_by_key(&code, |&(from, _)| from) {
            Ok(index) => result.extend(
                CANONICAL_DECOMPOSITION[index]
                    .1
                    .chars()
                    .filter(|&part| !in_ranges(NONSPACING_MARK, part)),
            ),
            Err(_) if in_ranges(NONSPACING_MARK, ch) => {}
            Err(_) => result.push(ch),
        }
    }
    result
}

/// tsgo `compareOrganizeImportsUnicodeKeys`.
fn compare_unicode_keys(a: &str, b: &str, numeric: bool) -> Ordering {
    if numeric {
        compare_strings_numeric(a, b)
    } else {
        a.cmp(b)
    }
}

/// tsgo `compareStringsNumeric`: digit runs by their values.
fn compare_strings_numeric(mut a: &str, mut b: &str) -> Ordering {
    while !a.is_empty() && !b.is_empty() {
        if a.as_bytes()[0].is_ascii_digit() && b.as_bytes()[0].is_ascii_digit() {
            let a_end = a
                .bytes()
                .position(|byte| !byte.is_ascii_digit())
                .unwrap_or(a.len());
            let b_end = b
                .bytes()
                .position(|byte| !byte.is_ascii_digit())
                .unwrap_or(b.len());
            let order = compare_numeric_text(&a[..a_end], &b[..b_end]);
            if order != Ordering::Equal {
                return order;
            }
            a = &a[a_end..];
            b = &b[b_end..];
            continue;
        }
        let a_char = a.chars().next().expect("not empty");
        let b_char = b.chars().next().expect("not empty");
        if a_char != b_char {
            return a_char.cmp(&b_char);
        }
        a = &a[a_char.len_utf8()..];
        b = &b[b_char.len_utf8()..];
    }
    a.len().cmp(&b.len())
}

/// tsgo `compareNumericText`.
fn compare_numeric_text(a: &str, b: &str) -> Ordering {
    let a_digits = match a.trim_start_matches('0') {
        "" => "0",
        digits => digits,
    };
    let b_digits = match b.trim_start_matches('0') {
        "" => "0",
        digits => digits,
    };
    a_digits
        .len()
        .cmp(&b_digits.len())
        .then_with(|| a_digits.cmp(b_digits))
        .then_with(|| a.cmp(b))
}

/// tsgo `compareOrganizeImportsCase`.
fn compare_case(a: &str, b: &str, case_first: OrganizeImportsCaseFirst) -> Ordering {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    for (&a_char, &b_char) in a_chars.iter().zip(&b_chars) {
        let a_upper = is_upper(a_char);
        let b_upper = is_upper(b_char);
        if a_upper != b_upper {
            let a_first = match case_first {
                OrganizeImportsCaseFirst::UPPER => a_upper,
                OrganizeImportsCaseFirst::LOWER => !a_upper,
                _ => !a_upper,
            };
            return if a_first {
                Ordering::Less
            } else {
                Ordering::Greater
            };
        }
    }
    a_chars.len().cmp(&b_chars.len())
}

/// The statements of a list that are import declarations (tsgo
/// `FilterImportDeclarations`).
pub(crate) fn filter_import_declarations(arena: &NodeArena, statements: &[NodeId]) -> Vec<NodeId> {
    statements
        .iter()
        .copied()
        .filter(|&statement| arena.node(statement).kind == SyntaxKind::ImportDeclaration)
        .collect()
}

/// tsgo `getModuleSpecifierExpression`.
fn module_specifier_expression(arena: &NodeArena, declaration: NodeId) -> Option<NodeId> {
    match &arena.node(declaration).data {
        NodeData::ImportEqualsDeclaration(data) => {
            let reference = data.module_reference?;
            match &arena.node(reference).data {
                NodeData::ExternalModuleReference(reference) => reference.expression,
                _ => None,
            }
        }
        NodeData::ImportDeclaration(data) => data.module_specifier,
        NodeData::VariableStatement(data) => {
            let list = data.declaration_list?;
            let NodeData::VariableDeclarationList(list) = &arena.node(list).data else {
                return None;
            };
            let first = *arena.node_array(list.declarations?).nodes.first()?;
            let NodeData::VariableDeclaration(declaration) = &arena.node(first).data else {
                return None;
            };
            let initializer = declaration.initializer?;
            let NodeData::CallExpression(call) = &arena.node(initializer).data else {
                return None;
            };
            arena.node_array(call.arguments?).nodes.first().copied()
        }
        _ => None,
    }
}

/// tsgo `GetExternalModuleName`: a string literal's text, else empty.
pub(crate) fn get_external_module_name(arena: &NodeArena, specifier: Option<NodeId>) -> String {
    match specifier.map(|specifier| &arena.node(specifier).data) {
        Some(NodeData::StringLiteral(data)) => data.text.to_string_lossy().into_owned(),
        Some(NodeData::NoSubstitutionTemplateLiteral(data)) => {
            data.text.to_string_lossy().into_owned()
        }
        _ => String::new(),
    }
}

/// tsgo `CompareModuleSpecifiers`: named ones first, external ones before
/// relative ones, then by name.
pub(crate) fn compare_module_specifiers(
    arena: &NodeArena,
    m1: Option<NodeId>,
    m2: Option<NodeId>,
    comparer: StringComparer,
) -> Ordering {
    let name1 = get_external_module_name(arena, m1);
    let name2 = get_external_module_name(arena, m2);
    name1
        .is_empty()
        .cmp(&name2.is_empty())
        .then_with(|| {
            is_external_module_name_relative(&name1).cmp(&is_external_module_name_relative(&name2))
        })
        .then_with(|| comparer.compare(&name1, &name2))
}

/// tsgo `tspath.IsExternalModuleNameRelative`.
fn is_external_module_name_relative(name: &str) -> bool {
    crate::references::is_external_module_name_relative(&tsc_types::JsString::from(name))
}

/// tsgo `getImportKindOrder`.
fn import_kind_order(arena: &NodeArena, statement: NodeId) -> u8 {
    match &arena.node(statement).data {
        NodeData::ImportDeclaration(data) => {
            let Some(clause) = data.import_clause else {
                return 0;
            };
            let NodeData::ImportClause(clause) = &arena.node(clause).data else {
                return 7;
            };
            if clause.is_type_only {
                1
            } else if clause
                .named_bindings
                .is_some_and(|bindings| arena.node(bindings).kind == SyntaxKind::NamespaceImport)
            {
                2
            } else if clause.name.is_some() {
                3
            } else {
                4
            }
        }
        NodeData::ImportEqualsDeclaration(_) => 5,
        NodeData::VariableStatement(_) => 6,
        _ => 7,
    }
}

/// tsgo `CompareImportsOrRequireStatements`.
pub(crate) fn compare_imports_or_require_statements(
    arena: &NodeArena,
    s1: NodeId,
    s2: NodeId,
    comparer: StringComparer,
) -> Ordering {
    compare_module_specifiers(
        arena,
        module_specifier_expression(arena, s1),
        module_specifier_expression(arena, s2),
        comparer,
    )
    .then_with(|| import_kind_order(arena, s1).cmp(&import_kind_order(arena, s2)))
}

/// An import or export specifier's name and whether it is type only.
fn specifier_name_and_type_only(arena: &NodeArena, specifier: NodeId) -> (String, bool) {
    let (name, is_type_only) = match &arena.node(specifier).data {
        NodeData::ImportSpecifier(data) => (data.name, data.is_type_only),
        NodeData::ExportSpecifier(data) => (data.name, data.is_type_only),
        // Organize imports compares source phase imports' clauses.
        NodeData::ImportClause(data) => (data.name, data.is_type_only),
        _ => (None, false),
    };
    let name = name.map_or_else(String::new, |name| node_name_text(arena, name));
    (name, is_type_only)
}

/// The text of a name (an identifier or a string literal).
pub(crate) fn node_name_text(arena: &NodeArena, name: NodeId) -> String {
    match &arena.node(name).data {
        NodeData::Identifier(data) => data.text().to_owned(),
        NodeData::StringLiteral(data) => data.text.to_string_lossy().into_owned(),
        _ => String::new(),
    }
}

/// tsgo `compareImportOrExportSpecifiers`.
pub(crate) fn compare_import_or_export_specifiers(
    arena: &NodeArena,
    s1: NodeId,
    s2: NodeId,
    comparer: StringComparer,
    type_order: OrganizeImportsTypeOrder,
) -> Ordering {
    let (name1, type_only1) = specifier_name_and_type_only(arena, s1);
    let (name2, type_only2) = specifier_name_and_type_only(arena, s2);
    match type_order {
        OrganizeImportsTypeOrder::FIRST => type_only2
            .cmp(&type_only1)
            .then_with(|| comparer.compare(&name1, &name2)),
        OrganizeImportsTypeOrder::INLINE => comparer.compare(&name1, &name2),
        _ => type_only1
            .cmp(&type_only2)
            .then_with(|| comparer.compare(&name1, &name2)),
    }
}

/// tsgo `GetNamedImportSpecifierComparer`: the comparer and type order of
/// specifiers (the preferences' comparer when none).
pub(crate) fn get_named_import_specifier_comparer(
    preferences: &UserPreferences,
    comparer: Option<StringComparer>,
) -> (StringComparer, OrganizeImportsTypeOrder) {
    let comparer = comparer.unwrap_or_else(|| {
        get_string_comparer(
            preferences,
            preferences.organize_imports_ignore_case == Some(true),
        )
    });
    (comparer, preferences.organize_imports_type_order)
}

/// tsgo `measureSortedness`: the adjacent pairs out of order.
fn measure_sortedness<T>(items: &[T], compare: impl Fn(&T, &T) -> Ordering) -> usize {
    items
        .windows(2)
        .filter(|pair| compare(&pair[0], &pair[1]) == Ordering::Greater)
        .count()
}

/// tsgo `detectCaseSensitivityBySort`: the comparer that leaves the groups
/// least out of order, and whether they are in order.
fn detect_case_sensitivity_by_sort(
    groups: &[Vec<String>],
    comparers: &[StringComparer],
) -> (StringComparer, bool) {
    let mut best = None;
    let mut best_diff = usize::MAX;
    for &comparer in comparers {
        let diff: usize = groups
            .iter()
            .filter(|group| group.len() > 1)
            .map(|group| measure_sortedness(group, |a, b| comparer.compare(a, b)))
            .sum();
        if diff < best_diff {
            best_diff = diff;
            best = Some(comparer);
        }
    }
    (best.unwrap_or(comparers[0]), best_diff == 0)
}

/// tsgo `DetectModuleSpecifierCaseBySort`.
pub(crate) fn detect_module_specifier_case_by_sort(
    arena: &NodeArena,
    groups: &[Vec<NodeId>],
    comparers: &[StringComparer],
) -> (StringComparer, bool) {
    let names: Vec<Vec<String>> = groups
        .iter()
        .map(|group| {
            group
                .iter()
                .map(|&declaration| {
                    get_external_module_name(arena, module_specifier_expression(arena, declaration))
                })
                .collect()
        })
        .collect();
    detect_case_sensitivity_by_sort(&names, comparers)
}

/// tsgo `namedImportSortResult`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct NamedImportSortResult {
    pub(crate) comparer: StringComparer,
    pub(crate) type_order: OrganizeImportsTypeOrder,
    pub(crate) is_sorted: bool,
}

/// tsgo `detectNamedImportOrganizationBySort`: the comparer and type order
/// that leave the named imports of the declarations least out of order.
pub(crate) fn detect_named_import_organization_by_sort(
    arena: &NodeArena,
    declarations: &[NodeId],
    comparers: &[StringComparer],
    type_orders: &[OrganizeImportsTypeOrder],
) -> Option<NamedImportSortResult> {
    let mut both_named_imports = false;
    let mut named_imports_by_declaration: Vec<Vec<NodeId>> = Vec::new();
    for &declaration in declarations {
        let NodeData::ImportDeclaration(data) = &arena.node(declaration).data else {
            continue;
        };
        let Some(clause) = data.import_clause else {
            continue;
        };
        let NodeData::ImportClause(clause) = &arena.node(clause).data else {
            continue;
        };
        let Some(bindings) = clause.named_bindings else {
            continue;
        };
        let NodeData::NamedImports(named) = &arena.node(bindings).data else {
            continue;
        };
        let elements = named
            .elements
            .map(|list| arena.node_array(list).nodes.to_vec())
            .unwrap_or_default();
        if elements.is_empty() {
            continue;
        }
        if !both_named_imports {
            let type_only = |element: &NodeId| specifier_name_and_type_only(arena, *element).1;
            if elements.iter().any(type_only) && elements.iter().any(|element| !type_only(element))
            {
                both_named_imports = true;
            }
        }
        named_imports_by_declaration.push(elements);
    }
    if named_imports_by_declaration.is_empty() {
        return None;
    }
    if !both_named_imports || type_orders.is_empty() {
        let names: Vec<Vec<String>> = named_imports_by_declaration
            .iter()
            .map(|elements| {
                elements
                    .iter()
                    .map(|&element| specifier_name_and_type_only(arena, element).0)
                    .collect()
            })
            .collect();
        let (comparer, is_sorted) = detect_case_sensitivity_by_sort(&names, comparers);
        let type_order = if type_orders.len() == 1 {
            type_orders[0]
        } else {
            OrganizeImportsTypeOrder::LAST
        };
        return Some(NamedImportSortResult {
            comparer,
            type_order,
            is_sorted,
        });
    }
    let orders = [
        OrganizeImportsTypeOrder::FIRST,
        OrganizeImportsTypeOrder::LAST,
        OrganizeImportsTypeOrder::INLINE,
    ];
    let slot = |order: OrganizeImportsTypeOrder| {
        orders
            .iter()
            .position(|&known| known == order)
            .expect("a known type order")
    };
    let mut best_diff = [usize::MAX; 3];
    let mut best_comparer = [comparers[0]; 3];
    for &comparer in comparers {
        let mut current_diff = [0usize; 3];
        for elements in &named_imports_by_declaration {
            for &type_order in type_orders {
                current_diff[slot(type_order)] += measure_sortedness(elements, |&a, &b| {
                    compare_import_or_export_specifiers(arena, a, b, comparer, type_order)
                });
            }
        }
        for &type_order in type_orders {
            let slot = slot(type_order);
            if current_diff[slot] < best_diff[slot] {
                best_diff[slot] = current_diff[slot];
                best_comparer[slot] = comparer;
            }
        }
    }
    for &best_type_order in type_orders {
        let best = best_diff[slot(best_type_order)];
        if type_orders
            .iter()
            .all(|&test| best_diff[slot(test)] >= best)
        {
            return Some(NamedImportSortResult {
                comparer: best_comparer[slot(best_type_order)],
                type_order: best_type_order,
                is_sorted: best == 0,
            });
        }
    }
    let last = slot(OrganizeImportsTypeOrder::LAST);
    Some(NamedImportSortResult {
        comparer: best_comparer[last],
        type_order: OrganizeImportsTypeOrder::LAST,
        is_sorted: best_diff[last] == 0,
    })
}

/// tsgo `core.BinarySearchUniqueFunc`'s index: where `compare` (an item
/// against the new one) finds it among the sorted items, or where it goes
/// (tsgo `GetImportSpecifierInsertionIndex`, `GetImportDeclarationInsertIndex`).
pub(crate) fn binary_search_unique<T>(items: &[T], compare: impl Fn(&T) -> Ordering) -> usize {
    if items.is_empty() {
        return 0;
    }
    let (mut low, mut high) = (0isize, items.len() as isize - 1);
    while low <= high {
        let middle = low + ((high - low) >> 1);
        match compare(&items[middle as usize]) {
            Ordering::Less => low = middle + 1,
            Ordering::Greater => high = middle - 1,
            Ordering::Equal => return middle as usize,
        }
    }
    low as usize
}

/// tsgo `getComparers`.
fn get_comparers(preferences: &UserPreferences) -> Vec<StringComparer> {
    if preferences.organize_imports_sort != OrganizeImportsSort::AUTO
        || preferences.organize_imports_ignore_case.is_some()
    {
        let ignore_case = preferences.organize_imports_ignore_case == Some(true);
        return vec![get_string_comparer(preferences, ignore_case)];
    }
    vec![
        get_string_comparer(preferences, true),
        get_string_comparer(preferences, false),
    ]
}

/// tsgo `GetOrganizeImportsStringComparerWithDetection`: the module
/// specifier comparer of the import statements' order, and whether they
/// are sorted.
pub(crate) fn get_organize_imports_string_comparer_with_detection(
    arena: &NodeArena,
    original_import_decls: &[NodeId],
    preferences: &UserPreferences,
) -> (StringComparer, bool) {
    detect_module_specifier_case_by_sort(
        arena,
        &[original_import_decls.to_vec()],
        &get_comparers(preferences),
    )
}

/// tsgo `GetNamedImportSpecifierComparerWithDetection`: the specifier
/// comparer of the order within an import declaration (or the file's), and
/// whether they are sorted (unknown when not detected).
pub(crate) fn get_named_import_specifier_comparer_with_detection(
    arena: &NodeArena,
    import_decl: NodeId,
    statements: Option<&[NodeId]>,
    preferences: &UserPreferences,
) -> ((StringComparer, OrganizeImportsTypeOrder), Option<bool>) {
    let (comparers, type_orders) = get_detection_lists(preferences);
    let import_stmt =
        (arena.node(import_decl).kind == SyntaxKind::ImportDeclaration).then_some(import_decl);
    let mut specifier_comparer =
        get_named_import_specifier_comparer(preferences, Some(comparers[0]));
    let mut is_sorted = None;
    if let Some(import_stmt) = import_stmt.filter(|_| {
        resolve_organize_imports_sort(preferences) == OrganizeImportsSort::AUTO
            || preferences.organize_imports_type_order == OrganizeImportsTypeOrder::AUTO
    }) {
        let detected = detect_named_import_organization_by_sort(
            arena,
            &[import_stmt],
            &comparers,
            &type_orders,
        )
        .or_else(|| {
            let statements = statements?;
            let all_imports = filter_import_declarations(arena, statements);
            detect_named_import_organization_by_sort(arena, &all_imports, &comparers, &type_orders)
        });
        if let Some(detected) = detected {
            is_sorted = Some(detected.is_sorted);
            specifier_comparer = (detected.comparer, detected.type_order);
        }
    }
    (specifier_comparer, is_sorted)
}
