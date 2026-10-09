//! tsgo `api/callbackfs.go` (19dadef8): a file system whose enabled
//! operations the client answers (`--callbacks`), the others and the
//! client's `null` falling back to the base file system. A failed call
//! panics, as in tsgo, and the request that read the file fails.

use std::collections::BTreeSet;
use std::io;
use std::sync::{Arc, OnceLock, Weak};
use std::time::SystemTime;

use serde::Deserialize;
use tsc_host::vfs::{DirEntry, Entries, FileSystem, FileType, Metadata};

use crate::ipc::Conn;

/// tsgo's callback names.
pub const CALLBACK_NAMES: [&str; 7] = [
    "readFile",
    "fileExists",
    "directoryExists",
    "getAccessibleEntries",
    "realpath",
    "writeFile",
    "removeFile",
];

/// tsgo `callbackFS`.
pub struct CallbackFs<F> {
    base: F,
    enabled: BTreeSet<&'static str>,
    /// tsgo `SetConnection`: set once the connection exists.
    conn: OnceLock<Weak<Conn>>,
}

impl<F: FileSystem> CallbackFs<F> {
    /// tsgo `newCallbackFS`; tsgo panics on an unknown name.
    pub fn new(base: F, callbacks: &[String]) -> Result<Self, String> {
        let enabled = callbacks
            .iter()
            .map(|name| {
                CALLBACK_NAMES
                    .iter()
                    .copied()
                    .find(|known| known == name)
                    .ok_or_else(|| format!("unknown callback name: {name}"))
            })
            .collect::<Result<_, _>>()?;
        Ok(Self {
            base,
            enabled,
            conn: OnceLock::new(),
        })
    }

    /// tsgo `SetConnection`.
    pub fn set_connection(&self, conn: &Arc<Conn>) {
        let _ = self.conn.set(Arc::downgrade(conn));
    }

    /// The client's answer to `name` with `argument` (JSON), when that
    /// callback is enabled and the answer is not `null`.
    fn call(&self, name: &str, argument: &str) -> Option<Vec<u8>> {
        if !self.enabled.contains(name) {
            return None;
        }
        let conn = self
            .conn
            .get()
            .and_then(Weak::upgrade)
            .unwrap_or_else(|| panic!("CallbackFS: {name} called before connection set"));
        let result = conn
            .call(name, argument)
            .unwrap_or_else(|error| panic!("{error}"));
        (!result.is_empty() && result != b"null").then_some(result)
    }

    fn quote(text: &str) -> String {
        serde_json::to_string(text).expect("a string serializes")
    }
}

impl<F: FileSystem> FileSystem for CallbackFs<F> {
    fn case_sensitive(&self) -> bool {
        self.base.case_sensitive()
    }

    /// tsgo `ReadFile`: `{ "content": ... }`, a `null` content for a missing
    /// file, and `null` for the base's answer.
    fn read(&self, path: &str) -> io::Result<Vec<u8>> {
        #[derive(Deserialize)]
        struct Wrapper {
            content: Option<String>,
        }
        match self.call("readFile", &Self::quote(path)) {
            Some(result) => {
                let wrapper: Wrapper =
                    serde_json::from_slice(&result).unwrap_or_else(|error| panic!("{error}"));
                wrapper
                    .content
                    .map(String::into_bytes)
                    .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
            }
            None => self.base.read(path),
        }
    }

    /// tsgo `Stat`: the base's.
    fn metadata(&self, path: &str) -> io::Result<Metadata> {
        self.base.metadata(path)
    }

    /// The listing `accessible_entries` gives, when the client lists
    /// directories.
    fn read_dir(&self, path: &str) -> io::Result<Vec<DirEntry>> {
        if !self.enabled.contains("getAccessibleEntries") {
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

    /// tsgo `Realpath`.
    fn canonicalize(&self, path: &str) -> io::Result<String> {
        match self.call("realpath", &Self::quote(path)) {
            Some(result) => {
                Ok(serde_json::from_slice(&result).unwrap_or_else(|error| panic!("{error}")))
            }
            None => self.base.canonicalize(path),
        }
    }

    /// tsgo `WriteFile`: `{ "path", "data" }`.
    fn write(&self, path: &str, contents: &[u8]) -> io::Result<()> {
        if !self.enabled.contains("writeFile") {
            return self.base.write(path, contents);
        }
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
        self.call_for_mutation("writeFile", &payload)
    }

    fn append(&self, path: &str, contents: &[u8]) -> io::Result<()> {
        self.base.append(path, contents)
    }

    fn create_dir_all(&self, path: &str) -> io::Result<()> {
        self.base.create_dir_all(path)
    }

    /// tsgo `Remove`.
    fn remove(&self, path: &str) -> io::Result<()> {
        if !self.enabled.contains("removeFile") {
            return self.base.remove(path);
        }
        self.call_for_mutation("removeFile", &Self::quote(path))
    }

    fn set_modified(&self, path: &str, modified: SystemTime) -> io::Result<()> {
        self.base.set_modified(path, modified)
    }

    /// tsgo `FileExists`: `true`, anything else is false, `null` the base's.
    fn is_file(&self, path: &str) -> bool {
        match self.call("fileExists", &Self::quote(path)) {
            Some(result) => result == b"true",
            None => self.base.is_file(path),
        }
    }

    /// tsgo `DirectoryExists`.
    fn is_dir(&self, path: &str) -> bool {
        match self.call("directoryExists", &Self::quote(path)) {
            Some(result) => result == b"true",
            None => self.base.is_dir(path),
        }
    }

    /// tsgo `GetAccessibleEntries`: `{ files, directories }`, `null` the
    /// base's.
    fn accessible_entries(&self, path: &str) -> Entries {
        #[derive(Deserialize)]
        struct RawEntries {
            #[serde(default)]
            files: Option<Vec<String>>,
            #[serde(default)]
            directories: Option<Vec<String>>,
        }
        match self.call("getAccessibleEntries", &Self::quote(path)) {
            Some(result) => {
                let raw: RawEntries =
                    serde_json::from_slice(&result).unwrap_or_else(|error| panic!("{error}"));
                Entries {
                    files: raw.files.unwrap_or_default(),
                    directories: raw.directories.unwrap_or_default(),
                    symlinks: BTreeSet::new(),
                }
            }
            None => self.base.accessible_entries(path),
        }
    }
}

impl<F: FileSystem> CallbackFs<F> {
    /// A write's or a removal's call: its error is the operation's.
    fn call_for_mutation(&self, name: &str, argument: &str) -> io::Result<()> {
        let conn = self.conn.get().and_then(Weak::upgrade).ok_or_else(|| {
            io::Error::other(format!("CallbackFS: {name} called before connection set"))
        })?;
        conn.call(name, argument)
            .map(|_| ())
            .map_err(|error| io::Error::other(error.to_string()))
    }
}
