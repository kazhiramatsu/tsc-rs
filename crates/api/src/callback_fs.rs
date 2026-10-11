//! tsgo `api/callbackfs.go` (aa8149273): a file system whose enabled
//! operations the client answers (`--callbacks`), the others falling back to
//! the base file system. Each answer is `{ "kind", "value" }`: a value, or
//! what the server does instead (`useOS`, `missing`, `identity`, `fakeStat`,
//! `noop`). A callback list entry `name:error` makes the operation fail, and
//! `realpath:identity`, `stat:fakeStat`, `writeFile:noop` and
//! `removeFile:noop` are the defaults of operations the client does not
//! answer. A failed call or a bad answer panics, as in tsgo, and the request
//! that read the file fails.

use std::collections::BTreeSet;
use std::io;
use std::sync::{Arc, OnceLock, Weak};
use std::time::{Duration, SystemTime};

use serde::Deserialize;
use tsc_host::vfs::{DirEntry, Entries, FileSystem, FileType, Metadata};

use crate::ipc::Conn;
use crate::request_fs::paths::{directory_path, normalized_absolute_path};

/// tsgo's callback names.
pub const CALLBACK_NAMES: [&str; 8] = [
    "readFile",
    "fileExists",
    "directoryExists",
    "getAccessibleEntries",
    "realpath",
    "stat",
    "writeFile",
    "removeFile",
];

/// tsgo's defaults for the operations the client does not answer.
const DEFAULTS: [&str; 4] = [
    "realpath:identity",
    "stat:fakeStat",
    "writeFile:noop",
    "removeFile:noop",
];

/// tsgo `callbackFS`.
pub struct CallbackFs<F> {
    base: F,
    enabled: BTreeSet<&'static str>,
    realpath_identity: bool,
    fake_stat: bool,
    write_file_noop: bool,
    remove_file_noop: bool,
    /// The operations configured with `:error`.
    error_callbacks: BTreeSet<&'static str>,
    /// `--useCaseSensitiveFileNames`, else the base's.
    case_sensitive: Option<bool>,
    /// tsgo `SetConnection`: set once the connection exists.
    conn: OnceLock<Weak<Conn>>,
}

/// tsgo `callbackResponse`.
#[derive(Deserialize)]
struct Response {
    #[serde(default)]
    kind: String,
    #[serde(default)]
    value: serde_json::Value,
}

fn known_name(name: &str) -> Result<&'static str, String> {
    CALLBACK_NAMES
        .iter()
        .copied()
        .find(|known| *known == name)
        .ok_or_else(|| format!("unknown callback name: {name}"))
}

impl<F: FileSystem> CallbackFs<F> {
    /// tsgo `newCallbackFS`; tsgo panics on an unknown name.
    pub fn new(
        base: F,
        callbacks: &[String],
        case_sensitive: Option<bool>,
    ) -> Result<Self, String> {
        let mut enabled = BTreeSet::new();
        let mut error_callbacks = BTreeSet::new();
        for callback in callbacks {
            if let Some(name) = callback.strip_suffix(":error") {
                error_callbacks.insert(known_name(name)?);
                continue;
            }
            if DEFAULTS.contains(&callback.as_str()) {
                continue;
            }
            enabled.insert(known_name(callback)?);
        }
        let has = |default: &str| callbacks.iter().any(|callback| callback == default);
        Ok(Self {
            base,
            enabled,
            realpath_identity: has("realpath:identity"),
            fake_stat: has("stat:fakeStat"),
            write_file_noop: has("writeFile:noop"),
            remove_file_noop: has("removeFile:noop"),
            error_callbacks,
            case_sensitive,
            conn: OnceLock::new(),
        })
    }

    /// tsgo `SetConnection`.
    pub fn set_connection(&self, conn: &Arc<Conn>) {
        let _ = self.conn.set(Arc::downgrade(conn));
    }

    fn is_enabled(&self, name: &str) -> bool {
        self.enabled.contains(name)
    }

    /// tsgo `panicIfError`.
    fn panic_if_error(&self, name: &str) {
        if self.error_callbacks.contains(name) {
            panic!("filesystem operation configured with serverFS.error: {name}");
        }
    }

    /// tsgo `call`: the client's raw answer, or the call's error.
    fn call_raw(&self, name: &str, argument: &str) -> Result<Vec<u8>, String> {
        let conn = self
            .conn
            .get()
            .and_then(Weak::upgrade)
            .ok_or_else(|| format!("CallbackFS: {name} called before connection set"))?;
        conn.call(name, argument).map_err(|error| error.to_string())
    }

    /// A read's call: a failed call panics, and the answer is decoded.
    fn call(&self, name: &str, argument: &str) -> Response {
        let result = self
            .call_raw(name, argument)
            .unwrap_or_else(|error| panic!("{error}"));
        decode_response(name, &result)
    }

    /// A write's or a removal's call: its error is the operation's.
    fn call_for_mutation(&self, name: &str, argument: &str) -> io::Result<Response> {
        let result = self.call_raw(name, argument).map_err(io::Error::other)?;
        Ok(decode_response(name, &result))
    }

    fn quote(text: &str) -> String {
        serde_json::to_string(text).expect("a string serializes")
    }

    /// tsgo `fakeStatForPath`: a directory or a file the existence checks
    /// find, with default metadata (no size, Go's zero time).
    fn fake_stat_for_path(&self, path: &str) -> io::Result<Metadata> {
        if self.is_dir(path) {
            return Ok(Metadata::new(FileType::Directory, 0, None));
        }
        if self.is_file(path) {
            return Ok(Metadata::new(FileType::File, 0, None));
        }
        Err(io::Error::from(io::ErrorKind::NotFound))
    }

    /// tsgo `WriteFile`: `{ "path", "data" }`; `useOS` and the base write
    /// through `base_write`.
    fn write_with(
        &self,
        path: &str,
        contents: &[u8],
        base_write: impl FnOnce() -> io::Result<()>,
    ) -> io::Result<()> {
        self.panic_if_error("writeFile");
        if self.is_enabled("writeFile") {
            #[derive(serde::Serialize)]
            struct Payload<'a> {
                path: &'a str,
                data: std::borrow::Cow<'a, str>,
            }
            let payload = serde_json::to_string(&Payload {
                path,
                data: String::from_utf8_lossy(contents),
            })
            .expect("the payload serializes");
            let response = self.call_for_mutation("writeFile", &payload)?;
            return match response.kind.as_str() {
                "value" | "noop" => Ok(()),
                "useOS" => base_write(),
                _ => invalid_response("writeFile", &response),
            };
        }
        if self.write_file_noop {
            return Ok(());
        }
        base_write()
    }
}

/// tsgo `decodeCallbackResponse`.
fn decode_response(name: &str, result: &[u8]) -> Response {
    let response: Response =
        serde_json::from_slice(result).unwrap_or_else(|error| panic!("{error}"));
    if response.kind.is_empty() {
        panic!("filesystem callback response is missing a kind");
    }
    if response.kind == "error" {
        panic!("filesystem callback returned serverFS.error: {name}");
    }
    response
}

/// tsgo `invalidCallbackResponse`.
fn invalid_response(name: &str, response: &Response) -> ! {
    panic!("invalid {name} callback response kind: {}", response.kind)
}

fn value_of<T: serde::de::DeserializeOwned>(response: Response) -> T {
    serde_json::from_value(response.value).unwrap_or_else(|error| panic!("{error}"))
}

impl<F: FileSystem> FileSystem for CallbackFs<F> {
    /// tsgo `CaseSensitivity`.
    fn case_sensitive(&self) -> bool {
        self.case_sensitive
            .unwrap_or_else(|| self.base.case_sensitive())
    }

    /// tsgo `ReadFile`.
    fn read(&self, path: &str) -> io::Result<Vec<u8>> {
        self.panic_if_error("readFile");
        if self.is_enabled("readFile") {
            let response = self.call("readFile", &Self::quote(path));
            return match response.kind.as_str() {
                "value" => Ok(value_of::<String>(response).into_bytes()),
                "missing" => Err(io::Error::from(io::ErrorKind::NotFound)),
                "useOS" => self.base.read(path),
                _ => invalid_response("readFile", &response),
            };
        }
        self.base.read(path)
    }

    /// tsgo `Stat`: the client's `{ mode, size, mtime }`, `missing`, a fake
    /// stat or the base's.
    fn metadata(&self, path: &str) -> io::Result<Metadata> {
        self.panic_if_error("stat");
        if self.is_enabled("stat") {
            #[derive(Deserialize)]
            struct Stat {
                mode: u32,
                size: i64,
                mtime: String,
            }
            let response = self.call("stat", &Self::quote(path));
            return match response.kind.as_str() {
                "value" => {
                    let stat: Stat = value_of(response);
                    let modified = parse_rfc3339(&stat.mtime)
                        .unwrap_or_else(|| panic!("parsing time {:?}", stat.mtime));
                    Ok(Metadata::new(
                        node_file_type(stat.mode),
                        u64::try_from(stat.size).unwrap_or(0),
                        Some(modified),
                    ))
                }
                "missing" => Err(io::Error::from(io::ErrorKind::NotFound)),
                "fakeStat" => self.fake_stat_for_path(path),
                "useOS" => self.base.metadata(path),
                _ => invalid_response("stat", &response),
            };
        }
        if self.fake_stat {
            return self.fake_stat_for_path(path);
        }
        self.base.metadata(path)
    }

    /// The listing `accessible_entries` gives, when the client lists
    /// directories.
    fn read_dir(&self, path: &str) -> io::Result<Vec<DirEntry>> {
        if !self.is_enabled("getAccessibleEntries") {
            return self.base.read_dir(path);
        }
        let entries = self.accessible_entries(path);
        Ok(entries
            .files
            .into_iter()
            .map(|name| DirEntry::new(name, FileType::File))
            .chain(
                entries
                    .directories
                    .into_iter()
                    .map(|name| DirEntry::new(name, FileType::Directory)),
            )
            .collect())
    }

    /// tsgo `Realpath`: a relative answer is resolved against the path's
    /// directory (`tspath.ToRootedPath`).
    fn canonicalize(&self, path: &str) -> io::Result<String> {
        self.panic_if_error("realpath");
        if self.is_enabled("realpath") {
            let response = self.call("realpath", &Self::quote(path));
            return match response.kind.as_str() {
                "value" => {
                    let real: String = value_of(response);
                    Ok(normalized_absolute_path(&real, &directory_path(path)))
                }
                "identity" => Ok(path.to_owned()),
                "useOS" => self.base.canonicalize(path),
                _ => invalid_response("realpath", &response),
            };
        }
        if self.realpath_identity {
            return Ok(path.to_owned());
        }
        self.base.canonicalize(path)
    }

    fn write(&self, path: &str, contents: &[u8]) -> io::Result<()> {
        self.write_with(path, contents, || self.base.write(path, contents))
    }

    /// tsgo `WriteFile`: the client writes the file, its directories
    /// included, and its error is the write's; `useOS` and the base write it
    /// with its directories.
    fn write_creating_dirs(&self, path: &str, contents: &[u8]) -> io::Result<()> {
        self.write_with(path, contents, || {
            self.base.write_creating_dirs(path, contents)
        })
    }

    fn append(&self, path: &str, contents: &[u8]) -> io::Result<()> {
        self.base.append(path, contents)
    }

    fn create_dir_all(&self, path: &str) -> io::Result<()> {
        self.base.create_dir_all(path)
    }

    /// tsgo `Remove`.
    fn remove(&self, path: &str) -> io::Result<()> {
        self.panic_if_error("removeFile");
        if self.is_enabled("removeFile") {
            let response = self.call_for_mutation("removeFile", &Self::quote(path))?;
            return match response.kind.as_str() {
                "value" | "noop" => Ok(()),
                "useOS" => self.base.remove(path),
                _ => invalid_response("removeFile", &response),
            };
        }
        if self.remove_file_noop {
            return Ok(());
        }
        self.base.remove(path)
    }

    fn set_modified(&self, path: &str, modified: SystemTime) -> io::Result<()> {
        self.base.set_modified(path, modified)
    }

    /// tsgo `FileExists`.
    fn is_file(&self, path: &str) -> bool {
        self.panic_if_error("fileExists");
        if self.is_enabled("fileExists") {
            let response = self.call("fileExists", &Self::quote(path));
            return match response.kind.as_str() {
                "value" => value_of(response),
                "useOS" => self.base.is_file(path),
                _ => invalid_response("fileExists", &response),
            };
        }
        self.base.is_file(path)
    }

    /// tsgo `DirectoryExists`.
    fn is_dir(&self, path: &str) -> bool {
        self.panic_if_error("directoryExists");
        if self.is_enabled("directoryExists") {
            let response = self.call("directoryExists", &Self::quote(path));
            return match response.kind.as_str() {
                "value" => value_of(response),
                "useOS" => self.base.is_dir(path),
                _ => invalid_response("directoryExists", &response),
            };
        }
        self.base.is_dir(path)
    }

    /// tsgo `GetAccessibleEntries`: `{ files, directories, symlinks }`.
    fn accessible_entries(&self, path: &str) -> Entries {
        #[derive(Deserialize)]
        struct RawEntries {
            #[serde(default)]
            files: Option<Vec<String>>,
            #[serde(default)]
            directories: Option<Vec<String>>,
            #[serde(default)]
            symlinks: Option<Vec<String>>,
        }
        self.panic_if_error("getAccessibleEntries");
        if self.is_enabled("getAccessibleEntries") {
            let response = self.call("getAccessibleEntries", &Self::quote(path));
            return match response.kind.as_str() {
                "value" => {
                    let raw: RawEntries = value_of(response);
                    Entries {
                        files: raw.files.unwrap_or_default(),
                        directories: raw.directories.unwrap_or_default(),
                        symlinks: raw.symlinks.unwrap_or_default().into_iter().collect(),
                    }
                }
                "useOS" => self.base.accessible_entries(path),
                _ => invalid_response("getAccessibleEntries", &response),
            };
        }
        self.base.accessible_entries(path)
    }
}

/// tsgo `nodeFileModeToGoFileMode`, as far as an entry's type goes: a
/// directory or a link, anything else a file.
fn node_file_type(mode: u32) -> FileType {
    match mode & 0o170000 {
        0o040000 => FileType::Directory,
        0o120000 => FileType::Symlink,
        _ => FileType::File,
    }
}

/// Go's `time.Parse(time.RFC3339Nano, text)`:
/// `YYYY-MM-DDTHH:MM:SS[.fraction](Z|±HH:MM)`.
fn parse_rfc3339(text: &str) -> Option<SystemTime> {
    let bytes = text.as_bytes();
    let number = |range: std::ops::Range<usize>| -> Option<i64> {
        let digits = text.get(range)?;
        if !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        digits.parse().ok()
    };
    if bytes.len() < 20
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || !matches!(bytes[10], b'T' | b't')
        || bytes[13] != b':'
        || bytes[16] != b':'
    {
        return None;
    }
    let (year, month, day) = (number(0..4)?, number(5..7)?, number(8..10)?);
    let (hour, minute, second) = (number(11..13)?, number(14..16)?, number(17..19)?);
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }
    let mut position = 19;
    let mut nanos = 0u32;
    if bytes.get(position) == Some(&b'.') {
        let start = position + 1;
        let end = start
            + bytes[start..]
                .iter()
                .take_while(|byte| byte.is_ascii_digit())
                .count();
        if end == start || end - start > 9 {
            return None;
        }
        let digits = &text[start..end];
        nanos = digits.parse::<u32>().ok()? * 10u32.pow(9 - u32::try_from(digits.len()).ok()?);
        position = end;
    }
    let offset_seconds = match bytes.get(position)? {
        b'Z' | b'z' if position + 1 == bytes.len() => 0,
        sign @ (b'+' | b'-') if position + 6 == bytes.len() && bytes[position + 3] == b':' => {
            let offset = number(position + 1..position + 3)? * 3600
                + number(position + 4..position + 6)? * 60;
            if *sign == b'-' {
                -offset
            } else {
                offset
            }
        }
        _ => return None,
    };
    // Days since 1970-01-01 of the civil date (Howard Hinnant's algorithm).
    let shifted_year = if month <= 2 { year - 1 } else { year };
    let era = shifted_year.div_euclid(400);
    let year_of_era = shifted_year - era * 400;
    let day_of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    let seconds = days * 86_400 + hour * 3600 + minute * 60 + second - offset_seconds;
    let since_epoch = Duration::new(seconds.unsigned_abs(), 0);
    let whole = if seconds >= 0 {
        SystemTime::UNIX_EPOCH.checked_add(since_epoch)?
    } else {
        SystemTime::UNIX_EPOCH.checked_sub(since_epoch)?
    };
    whole.checked_add(Duration::from_nanos(u64::from(nanos)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3339_times_parse_like_go() {
        let at = |seconds: u64, nanos: u32| SystemTime::UNIX_EPOCH + Duration::new(seconds, nanos);
        assert_eq!(parse_rfc3339("1970-01-01T00:00:00Z"), Some(at(0, 0)));
        assert_eq!(
            parse_rfc3339("2024-01-02T03:04:05.000Z"),
            Some(at(1_704_164_645, 0))
        );
        assert_eq!(
            parse_rfc3339("2024-01-02T03:04:05.5Z"),
            Some(at(1_704_164_645, 500_000_000))
        );
        assert_eq!(
            parse_rfc3339("2024-01-02T12:04:05+09:00"),
            Some(at(1_704_164_645, 0))
        );
        assert_eq!(parse_rfc3339("2024-01-02 03:04:05Z"), None);
        assert_eq!(parse_rfc3339("2024-13-02T03:04:05Z"), None);
    }

    // tsgo TestNodeFileModeToGoFileMode, as far as an entry's type goes.
    #[test]
    fn node_modes_map_to_entry_types() {
        assert_eq!(node_file_type(0o040755), FileType::Directory);
        assert_eq!(node_file_type(0o100644), FileType::File);
        assert_eq!(node_file_type(0o120777), FileType::Symlink);
        assert_eq!(node_file_type(0o010600), FileType::File);
        assert_eq!(node_file_type(0o140600), FileType::File);
    }
}
