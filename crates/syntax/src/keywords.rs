// tsgo textToKeyword, with a collision-free, length-indexed hash.
// Seeds are independently searched for this keyword set. Compile-time table
// construction rejects collisions; the final byte comparison rejects all
// non-keywords, including Unicode identifiers and same-prefix misspellings.
use crate::SyntaxKind;

const KEYWORDS: &[(&str, SyntaxKind)] = &[
    ("abstract", SyntaxKind::AbstractKeyword),
    ("accessor", SyntaxKind::AccessorKeyword),
    ("any", SyntaxKind::AnyKeyword),
    ("as", SyntaxKind::AsKeyword),
    ("assert", SyntaxKind::AssertKeyword),
    ("asserts", SyntaxKind::AssertsKeyword),
    ("async", SyntaxKind::AsyncKeyword),
    ("await", SyntaxKind::AwaitKeyword),
    ("bigint", SyntaxKind::BigIntKeyword),
    ("boolean", SyntaxKind::BooleanKeyword),
    ("break", SyntaxKind::BreakKeyword),
    ("case", SyntaxKind::CaseKeyword),
    ("catch", SyntaxKind::CatchKeyword),
    ("class", SyntaxKind::ClassKeyword),
    ("const", SyntaxKind::ConstKeyword),
    ("constructor", SyntaxKind::ConstructorKeyword),
    ("continue", SyntaxKind::ContinueKeyword),
    ("debugger", SyntaxKind::DebuggerKeyword),
    ("declare", SyntaxKind::DeclareKeyword),
    ("default", SyntaxKind::DefaultKeyword),
    ("defer", SyntaxKind::DeferKeyword),
    ("delete", SyntaxKind::DeleteKeyword),
    ("do", SyntaxKind::DoKeyword),
    ("else", SyntaxKind::ElseKeyword),
    ("enum", SyntaxKind::EnumKeyword),
    ("export", SyntaxKind::ExportKeyword),
    ("extends", SyntaxKind::ExtendsKeyword),
    ("false", SyntaxKind::FalseKeyword),
    ("finally", SyntaxKind::FinallyKeyword),
    ("for", SyntaxKind::ForKeyword),
    ("from", SyntaxKind::FromKeyword),
    ("function", SyntaxKind::FunctionKeyword),
    ("get", SyntaxKind::GetKeyword),
    ("global", SyntaxKind::GlobalKeyword),
    ("if", SyntaxKind::IfKeyword),
    ("implements", SyntaxKind::ImplementsKeyword),
    ("import", SyntaxKind::ImportKeyword),
    ("in", SyntaxKind::InKeyword),
    ("infer", SyntaxKind::InferKeyword),
    ("instanceof", SyntaxKind::InstanceOfKeyword),
    ("interface", SyntaxKind::InterfaceKeyword),
    ("intrinsic", SyntaxKind::IntrinsicKeyword),
    ("is", SyntaxKind::IsKeyword),
    ("keyof", SyntaxKind::KeyOfKeyword),
    ("let", SyntaxKind::LetKeyword),
    ("module", SyntaxKind::ModuleKeyword),
    ("namespace", SyntaxKind::NamespaceKeyword),
    ("never", SyntaxKind::NeverKeyword),
    ("new", SyntaxKind::NewKeyword),
    ("null", SyntaxKind::NullKeyword),
    ("number", SyntaxKind::NumberKeyword),
    ("object", SyntaxKind::ObjectKeyword),
    ("of", SyntaxKind::OfKeyword),
    ("out", SyntaxKind::OutKeyword),
    ("override", SyntaxKind::OverrideKeyword),
    ("package", SyntaxKind::PackageKeyword),
    ("private", SyntaxKind::PrivateKeyword),
    ("protected", SyntaxKind::ProtectedKeyword),
    ("public", SyntaxKind::PublicKeyword),
    ("readonly", SyntaxKind::ReadonlyKeyword),
    ("require", SyntaxKind::RequireKeyword),
    ("return", SyntaxKind::ReturnKeyword),
    ("satisfies", SyntaxKind::SatisfiesKeyword),
    ("set", SyntaxKind::SetKeyword),
    ("source", SyntaxKind::SourceKeyword),
    ("static", SyntaxKind::StaticKeyword),
    ("string", SyntaxKind::StringKeyword),
    ("super", SyntaxKind::SuperKeyword),
    ("switch", SyntaxKind::SwitchKeyword),
    ("symbol", SyntaxKind::SymbolKeyword),
    ("this", SyntaxKind::ThisKeyword),
    ("throw", SyntaxKind::ThrowKeyword),
    ("true", SyntaxKind::TrueKeyword),
    ("try", SyntaxKind::TryKeyword),
    ("type", SyntaxKind::TypeKeyword),
    ("typeof", SyntaxKind::TypeOfKeyword),
    ("undefined", SyntaxKind::UndefinedKeyword),
    ("unique", SyntaxKind::UniqueKeyword),
    ("unknown", SyntaxKind::UnknownKeyword),
    ("using", SyntaxKind::UsingKeyword),
    ("var", SyntaxKind::VarKeyword),
    ("void", SyntaxKind::VoidKeyword),
    ("while", SyntaxKind::WhileKeyword),
    ("with", SyntaxKind::WithKeyword),
    ("yield", SyntaxKind::YieldKeyword),
];

const MULTIPLIERS: [u32; 12] = [
    0x00000000, 0x00000000, 0x34131339, 0x2fefa5fd, 0x50ba21c1, 0x1db3b0e7, 0xb68d5f79, 0x8e80c45d,
    0xd172aa39, 0x2ece0341, 0x79d29c8d, 0x73b6dfc3,
];

#[inline]
const fn keyword_slot(bytes: &[u8]) -> usize {
    let length = bytes.len();
    let key = bytes[0] as u32 | ((bytes[1] as u32) << 8) | ((bytes[length - 1] as u32) << 16);
    ((length - 2) << 6) | ((key.wrapping_mul(MULTIPLIERS[length]) >> 26) as usize)
}

const KEYWORD_SLOTS: [u8; 10 * 64] = {
    let mut slots = [0; 10 * 64];
    let mut index = 0;
    while index < KEYWORDS.len() {
        let slot = keyword_slot(KEYWORDS[index].0.as_bytes());
        assert!(slots[slot] == 0, "keyword hash collision");
        slots[slot] = index as u8 + 1;
        index += 1;
    }
    slots
};

#[inline]
pub(crate) fn keyword_kind(text: &str) -> Option<SyntaxKind> {
    let bytes = text.as_bytes();
    if !(2..=11).contains(&bytes.len()) {
        return None;
    }
    let index = KEYWORD_SLOTS[keyword_slot(bytes)];
    if index == 0 {
        return None;
    }
    let (word, kind) = KEYWORDS[index as usize - 1];
    (bytes == word.as_bytes()).then_some(kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_keyword_and_near_miss() {
        // Literal expected numeric kinds retain the original mapping independently
        // of the lookup table and its hash construction.
        let expected: &[(&str, u16)] = &[
            ("abstract", 128),
            ("accessor", 129),
            ("any", 133),
            ("as", 130),
            ("assert", 132),
            ("asserts", 131),
            ("async", 134),
            ("await", 135),
            ("bigint", 163),
            ("boolean", 136),
            ("break", 83),
            ("case", 84),
            ("catch", 85),
            ("class", 86),
            ("const", 87),
            ("constructor", 137),
            ("continue", 88),
            ("debugger", 89),
            ("declare", 138),
            ("default", 90),
            ("defer", 166),
            ("delete", 91),
            ("do", 92),
            ("else", 93),
            ("enum", 94),
            ("export", 95),
            ("extends", 96),
            ("false", 97),
            ("finally", 98),
            ("for", 99),
            ("from", 161),
            ("function", 100),
            ("get", 139),
            ("global", 162),
            ("if", 101),
            ("implements", 119),
            ("import", 102),
            ("in", 103),
            ("infer", 140),
            ("instanceof", 104),
            ("interface", 120),
            ("intrinsic", 141),
            ("is", 142),
            ("keyof", 143),
            ("let", 121),
            ("module", 144),
            ("namespace", 145),
            ("never", 146),
            ("new", 105),
            ("null", 106),
            ("number", 150),
            ("object", 151),
            ("of", 165),
            ("out", 147),
            ("override", 164),
            ("package", 122),
            ("private", 123),
            ("protected", 124),
            ("public", 125),
            ("readonly", 148),
            ("require", 149),
            ("return", 107),
            ("satisfies", 152),
            ("set", 153),
            ("source", 167),
            ("static", 126),
            ("string", 154),
            ("super", 108),
            ("switch", 109),
            ("symbol", 155),
            ("this", 110),
            ("throw", 111),
            ("true", 112),
            ("try", 113),
            ("type", 156),
            ("typeof", 114),
            ("undefined", 157),
            ("unique", 158),
            ("unknown", 159),
            ("using", 160),
            ("var", 115),
            ("void", 116),
            ("while", 117),
            ("with", 118),
            ("yield", 127),
        ];
        for &(word, kind) in expected {
            assert_eq!(keyword_kind(word).map(|k| k as u16), Some(kind), "{word}");
            assert_eq!(keyword_kind(&word.to_uppercase()), None);
            for suffix in ["_", "0", "\0", "é"] {
                assert_eq!(keyword_kind(&format!("{word}{suffix}")), None);
                assert_eq!(keyword_kind(&format!("{suffix}{word}")), None);
            }
        }
        for word in [
            "",
            "a",
            "i",
            "ofx",
            "typeofx",
            "constructo",
            "constructorx",
            "😀",
            "éé",
        ] {
            assert_eq!(keyword_kind(word), None, "{word:?}");
        }
    }
}
