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
        let Some(dot) = file_name.rfind('.') else {
            return Self::Unknown;
        };
        match file_name[dot..].to_ascii_lowercase().as_str() {
            ".js" | ".cjs" | ".mjs" => Self::Js,
            ".jsx" => Self::Jsx,
            ".ts" | ".cts" | ".mts" => Self::Ts,
            ".tsx" => Self::Tsx,
            ".json" => Self::Json,
            _ => Self::Unknown,
        }
    }
}
