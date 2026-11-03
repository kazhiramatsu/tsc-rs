// Generated from tsc 6.0.3 by `cargo xtask codegen scanner`, which went with
// the 6.0.3 profile (b27eb13bb); edited by hand since, following tsgo's
// textToKeyword.

use crate::SyntaxKind;

/// tsgo textToKeyword: the keyword `text` spells. Every keyword is 2 to 11
/// lowercase ASCII letters, and the match goes by the first letter, so an
/// identifier is compared with at most nine keywords.
pub(crate) fn keyword_kind(text: &str) -> Option<SyntaxKind> {
    let bytes = text.as_bytes();
    if !(2..=11).contains(&bytes.len()) {
        return None;
    }
    let kind = match bytes[0] {
        b'a' => match text {
            "abstract" => 128,
            "accessor" => 129,
            "any" => 133,
            "as" => 130,
            "assert" => 132,
            "asserts" => 131,
            "async" => 134,
            "await" => 135,
            _ => return None,
        },
        b'b' => match text {
            "bigint" => 163,
            "boolean" => 136,
            "break" => 83,
            _ => return None,
        },
        b'c' => match text {
            "case" => 84,
            "catch" => 85,
            "class" => 86,
            "const" => 87,
            "constructor" => 137,
            "continue" => 88,
            _ => return None,
        },
        b'd' => match text {
            "debugger" => 89,
            "declare" => 138,
            "default" => 90,
            "defer" => 166,
            "delete" => 91,
            "do" => 92,
            _ => return None,
        },
        b'e' => match text {
            "else" => 93,
            "enum" => 94,
            "export" => 95,
            "extends" => 96,
            _ => return None,
        },
        b'f' => match text {
            "false" => 97,
            "finally" => 98,
            "for" => 99,
            "from" => 161,
            "function" => 100,
            _ => return None,
        },
        b'g' => match text {
            "get" => 139,
            "global" => 162,
            _ => return None,
        },
        b'i' => match text {
            "if" => 101,
            "implements" => 119,
            "import" => 102,
            "in" => 103,
            "infer" => 140,
            "instanceof" => 104,
            "interface" => 120,
            "intrinsic" => 141,
            "is" => 142,
            _ => return None,
        },
        b'k' => match text {
            "keyof" => 143,
            _ => return None,
        },
        b'l' => match text {
            "let" => 121,
            _ => return None,
        },
        b'm' => match text {
            "module" => 144,
            _ => return None,
        },
        b'n' => match text {
            "namespace" => 145,
            "never" => 146,
            "new" => 105,
            "null" => 106,
            "number" => 150,
            _ => return None,
        },
        b'o' => match text {
            "object" => 151,
            "of" => 165,
            "out" => 147,
            "override" => 164,
            _ => return None,
        },
        b'p' => match text {
            "package" => 122,
            "private" => 123,
            "protected" => 124,
            "public" => 125,
            _ => return None,
        },
        b'r' => match text {
            "readonly" => 148,
            "require" => 149,
            "return" => 107,
            _ => return None,
        },
        b's' => match text {
            "satisfies" => 152,
            "set" => 153,
            "source" => 167,
            "static" => 126,
            "string" => 154,
            "super" => 108,
            "switch" => 109,
            "symbol" => 155,
            _ => return None,
        },
        b't' => match text {
            "this" => 110,
            "throw" => 111,
            "true" => 112,
            "try" => 113,
            "type" => 156,
            "typeof" => 114,
            _ => return None,
        },
        b'u' => match text {
            "undefined" => 157,
            "unique" => 158,
            "unknown" => 159,
            "using" => 160,
            _ => return None,
        },
        b'v' => match text {
            "var" => 115,
            "void" => 116,
            _ => return None,
        },
        b'w' => match text {
            "while" => 117,
            "with" => 118,
            _ => return None,
        },
        b'y' => match text {
            "yield" => 127,
            _ => return None,
        },
        _ => return None,
    };
    SyntaxKind::from_u16(kind)
}
