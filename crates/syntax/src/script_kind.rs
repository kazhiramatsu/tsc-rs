//! tsgo `core.ScriptKind`: the kind of source a file is parsed as.

/// tsgo `core.ScriptKind`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ScriptKind {
    #[default]
    Unknown = 0,
    Js = 1,
    Jsx = 2,
    Ts = 3,
    Tsx = 4,
    Json = 6,
}

impl ScriptKind {
    /// The kind of tsgo's `core.ScriptKind` number among those a client may
    /// create a source file of (JS, JSX, TS, TSX, JSON).
    pub fn from_number(number: u32) -> Option<Self> {
        match number {
            1 => Some(Self::Js),
            2 => Some(Self::Jsx),
            3 => Some(Self::Ts),
            4 => Some(Self::Tsx),
            6 => Some(Self::Json),
            _ => None,
        }
    }

    /// tsgo `core.EnsureScriptKindFromFileName`: the file name's kind, and
    /// TypeScript for a name without a known extension.
    pub fn ensure_from_file_name(file_name: &str) -> Self {
        match Self::from_file_name(file_name) {
            Self::Unknown => Self::Ts,
            kind => kind,
        }
    }

    /// tsgo `core.GetScriptKindFromFileName`.
    pub fn from_file_name(file_name: &str) -> Self {
        Self::from_file_name_bytes(file_name.as_bytes())
    }

    /// [`Self::ensure_from_file_name`] over a name's bytes.
    pub fn ensure_from_file_name_bytes(file_name: &[u8]) -> Self {
        match Self::from_file_name_bytes(file_name) {
            Self::Unknown => Self::Ts,
            kind => kind,
        }
    }

    /// [`Self::from_file_name`] over a name's bytes, UTF-8 or not: the
    /// extension starts at the last `.` (a byte no other character's
    /// encoding contains) and is compared without regard to ASCII case. A
    /// name is asked for its kind once a source and more; no text is made
    /// for the answer.
    pub fn from_file_name_bytes(file_name: &[u8]) -> Self {
        let Some(dot) = file_name.iter().rposition(|&byte| byte == b'.') else {
            return Self::Unknown;
        };
        let extension = &file_name[dot + 1..];
        const KINDS: [(&[u8], ScriptKind); 9] = [
            (b"ts", ScriptKind::Ts),
            (b"tsx", ScriptKind::Tsx),
            (b"js", ScriptKind::Js),
            (b"jsx", ScriptKind::Jsx),
            (b"json", ScriptKind::Json),
            (b"mts", ScriptKind::Ts),
            (b"cts", ScriptKind::Ts),
            (b"mjs", ScriptKind::Js),
            (b"cjs", ScriptKind::Js),
        ];
        KINDS
            .iter()
            .find(|(known, _)| extension.eq_ignore_ascii_case(known))
            .map_or(Self::Unknown, |&(_, kind)| kind)
    }
}

#[cfg(test)]
mod tests {
    use super::ScriptKind;

    /// The answer for a name is the lower-cased extension's, as before; a
    /// name without a dot, with a trailing dot or with an unknown extension
    /// has no kind.
    #[test]
    fn a_names_kind_is_its_extensions_whatever_the_case() {
        let cases = [
            ("a.ts", ScriptKind::Ts),
            ("a.d.ts", ScriptKind::Ts),
            ("A.TS", ScriptKind::Ts),
            ("a.Tsx", ScriptKind::Tsx),
            ("dir.x/a.mts", ScriptKind::Ts),
            ("a.cts", ScriptKind::Ts),
            ("a.js", ScriptKind::Js),
            ("a.MJS", ScriptKind::Js),
            ("a.cjs", ScriptKind::Js),
            ("a.jsx", ScriptKind::Jsx),
            ("package.JSON", ScriptKind::Json),
            ("a", ScriptKind::Unknown),
            ("a.", ScriptKind::Unknown),
            ("a.tsx.map", ScriptKind::Unknown),
            ("a.ts/b", ScriptKind::Unknown),
            ("\u{212a}.ts", ScriptKind::Ts),
            ("a.t\u{17f}", ScriptKind::Unknown),
        ];
        for (name, kind) in cases {
            assert_eq!(ScriptKind::from_file_name(name), kind, "{name}");
            // What the function was before: the lower-cased text after the
            // last dot, matched as a whole.
            let expected = name.rfind('.').map_or(ScriptKind::Unknown, |dot| {
                match name[dot..].to_ascii_lowercase().as_str() {
                    ".js" | ".cjs" | ".mjs" => ScriptKind::Js,
                    ".jsx" => ScriptKind::Jsx,
                    ".ts" | ".cts" | ".mts" => ScriptKind::Ts,
                    ".tsx" => ScriptKind::Tsx,
                    ".json" => ScriptKind::Json,
                    _ => ScriptKind::Unknown,
                }
            });
            assert_eq!(kind, expected, "{name}");
        }
        assert_eq!(ScriptKind::ensure_from_file_name("a"), ScriptKind::Ts);
        assert_eq!(
            ScriptKind::ensure_from_file_name_bytes(b"a.\xed\xa0\x80"),
            ScriptKind::Ts
        );
        assert_eq!(
            ScriptKind::from_file_name_bytes(b"\xed\xa0\x80.jsx"),
            ScriptKind::Jsx
        );
    }
}
