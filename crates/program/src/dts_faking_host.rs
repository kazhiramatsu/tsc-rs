//! tsgo `projectReferenceDtsFakingHost` (compiler/
//! projectreferencedtsfakinghost.go): the module resolution host of a
//! program that reads the sources of its referenced projects. An output
//! declaration file of such a project exists for the resolver when its
//! source does, and so does a directory that holds or lies inside the
//! project's declaration directory, so an import reaches an output the
//! project has not built (the loader then reads the source in its place).
//! A `node_modules` package directory that links elsewhere is followed
//! through the link. Reads and every other query are the host's.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tsc_diagnostics::{JsStr, JsString};
use tsc_host::{CompilerHost, DirectoryListingEntry, HostError};

use crate::js_path::{file_name_key, normalize_path, normalized_absolute_path};
use crate::module_requests::is_declaration_file_name;
use crate::path::CanonicalPath;
use crate::project_references::ResolvedProjectReferences;

pub(crate) struct DtsFakingHost<'a> {
    host: &'a dyn CompilerHost,
    references: Arc<ResolvedProjectReferences>,
    current_directory: JsString,
    case_sensitive: bool,
    /// tsgo `dtsDirectories`: each referenced project's `declarationDir`,
    /// else its `outDir`, as a path.
    dts_directories: Vec<String>,
    known: RefCell<KnownSymlinks>,
}

/// tsgo `KnownSymlinks`, the two maps the faking host uses.
#[derive(Default)]
struct KnownSymlinks {
    /// A linked directory's path (with a trailing separator): its real
    /// directory's name and path (each with a trailing separator).
    directories: BTreeMap<String, (String, String)>,
    /// A file found through a linked directory, by path: its real name.
    files: BTreeMap<String, String>,
}

impl<'a> DtsFakingHost<'a> {
    /// The host over `host` for a program whose referenced projects have
    /// outputs (tsgo `initMapper`); `root_config` is the program's own
    /// config, whose directories are not declaration directories.
    pub(crate) fn new(
        host: &'a dyn CompilerHost,
        references: Arc<ResolvedProjectReferences>,
        root_config: Option<&CanonicalPath>,
    ) -> Result<Self, HostError> {
        let current_directory = host.current_directory_js()?;
        let case_sensitive = host.use_case_sensitive_file_names();
        let dts_directories = references
            .projects()
            .filter(|project| Some(project.canonical()) != root_config)
            .filter_map(|project| {
                let options = project.compiler_options();
                options
                    .declaration_dir
                    .as_ref()
                    .filter(|directory| !directory.is_empty())
                    .or_else(|| {
                        options
                            .out_dir
                            .as_ref()
                            .filter(|directory| !directory.is_empty())
                    })
                    .map(|directory| {
                        to_path(directory.as_js(), current_directory.as_js(), case_sensitive)
                    })
            })
            .collect();
        Ok(Self {
            host,
            references,
            current_directory,
            case_sensitive,
            dts_directories,
            known: RefCell::new(KnownSymlinks::default()),
        })
    }

    fn to_path(&self, path: JsStr<'_>) -> String {
        to_path(path, self.current_directory.as_js(), self.case_sensitive)
    }

    /// tsgo `fileExistsIfProjectReferenceDts`: whether the source of the
    /// output declaration file `file` exists, or unknown for another file.
    fn file_exists_if_project_reference_dts(
        &self,
        file: JsStr<'_>,
    ) -> Result<Option<bool>, HostError> {
        let Ok(path) = CanonicalPath::from_js_normalized(JsStr::from_str(&self.to_path(file)))
        else {
            return Ok(None);
        };
        match self.references.source_for_output(&path) {
            Some(output) => self.host.file_exists_js(output.source()).map(Some),
            None => Ok(None),
        }
    }

    /// tsgo `directoryExistsIfProjectReferenceDeclDir`: a directory that
    /// holds or lies inside a declaration directory exists, any other is
    /// unknown.
    fn directory_exists_if_project_reference_decl_dir(&self, directory: JsStr<'_>) -> Option<bool> {
        let path = self.to_path(directory);
        self.dts_directories
            .iter()
            .any(|declaration_directory| {
                contains_path(&path, declaration_directory)
                    || contains_path(declaration_directory, &path)
            })
            .then_some(true)
    }

    fn exists_if_project_reference_output(
        &self,
        file_or_directory: JsStr<'_>,
        is_file: bool,
    ) -> Result<Option<bool>, HostError> {
        if is_file {
            self.file_exists_if_project_reference_dts(file_or_directory)
        } else {
            Ok(self.directory_exists_if_project_reference_decl_dir(file_or_directory))
        }
    }

    /// tsgo `fileOrDirectoryExistsUsingSource`.
    fn exists_using_source(
        &self,
        file_or_directory: JsStr<'_>,
        is_file: bool,
    ) -> Result<bool, HostError> {
        if let Some(result) = self.exists_if_project_reference_output(file_or_directory, is_file)? {
            return Ok(result);
        }
        let path = self.to_path(file_or_directory);
        if !path.contains("/node_modules/") {
            return Ok(false);
        }
        // A symlinked package directory may lead to a referenced project.
        if let Some(package_root) = parse_node_module_from_path(file_or_directory) {
            self.handle_directory_could_be_symlink(JsStr::from_str(&package_root))?;
        }
        let links = {
            let known = self.known.borrow();
            if known.directories.is_empty() {
                return Ok(false);
            }
            if is_file && known.files.contains_key(&path) {
                return Ok(true);
            }
            known
                .directories
                .iter()
                .map(|(directory, (real, real_path))| {
                    (directory.clone(), real.clone(), real_path.clone())
                })
                .collect::<Vec<_>>()
        };
        for (directory_path, real, real_path) in links {
            let Some(relative) = path.strip_prefix(&directory_path) else {
                continue;
            };
            let target = format!("{real_path}{relative}");
            if self.exists_if_project_reference_output(JsStr::from_str(&target), is_file)?
                == Some(true)
            {
                if is_file {
                    // The file's real name, for the resolver's realpath.
                    let absolute =
                        normalized_absolute_path(file_or_directory, self.current_directory.as_js())
                            .to_string_lossy()
                            .into_owned();
                    let real_file = format!(
                        "{real}{}",
                        absolute.get(directory_path.len()..).unwrap_or_default()
                    );
                    self.known.borrow_mut().files.insert(path, real_file);
                }
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// tsgo `handleDirectoryCouldBeSymlink`: remember a `node_modules`
    /// directory that links elsewhere.
    fn handle_directory_could_be_symlink(&self, directory: JsStr<'_>) -> Result<(), HostError> {
        if contains_ignored_path(directory) || !directory.contains("/node_modules/") {
            return Ok(());
        }
        let directory_path = with_trailing_separator(self.to_path(directory));
        if self
            .known
            .borrow()
            .directories
            .contains_key(&directory_path)
        {
            return Ok(());
        }
        let Some(real_directory) = self.realpath_js(directory)? else {
            return Ok(());
        };
        if real_directory.as_js() == directory {
            return Ok(());
        }
        let real_path = with_trailing_separator(self.to_path(real_directory.as_js()));
        if real_path == directory_path {
            return Ok(());
        }
        let real_directory = with_trailing_separator(real_directory.to_string_lossy().into_owned());
        self.known
            .borrow_mut()
            .directories
            .insert(directory_path, (real_directory, real_path));
        Ok(())
    }
}

impl CompilerHost for DtsFakingHost<'_> {
    fn current_directory_js(&self) -> Result<JsString, HostError> {
        self.host.current_directory_js()
    }

    fn read_file_js(&self, path: JsStr<'_>) -> Result<Option<Vec<u8>>, HostError> {
        self.host.read_file_js(path)
    }

    fn file_size_hint_js(&self, path: JsStr<'_>) -> Result<Option<u64>, HostError> {
        self.host.file_size_hint_js(path)
    }

    /// An output declaration file of a referenced project exists when its
    /// source does.
    fn file_exists_js(&self, path: JsStr<'_>) -> Result<bool, HostError> {
        if self.host.file_exists_js(path)? {
            return Ok(true);
        }
        if !is_declaration_file_name(path) {
            return Ok(false);
        }
        self.exists_using_source(path, true)
    }

    /// A directory that holds or lies inside a declaration directory of a
    /// referenced project exists.
    fn directory_exists_js(&self, path: JsStr<'_>) -> Result<bool, HostError> {
        if self.host.directory_exists_js(path)? {
            self.handle_directory_could_be_symlink(path)?;
            return Ok(true);
        }
        self.exists_using_source(path, false)
    }

    fn read_directory_js(&self, path: JsStr<'_>) -> Result<Vec<JsString>, HostError> {
        self.host.read_directory_js(path)
    }

    fn read_directory_listing_js(
        &self,
        path: JsStr<'_>,
    ) -> Result<Vec<DirectoryListingEntry>, HostError> {
        self.host.read_directory_listing_js(path)
    }

    fn get_directories_js(&self, path: JsStr<'_>) -> Result<Vec<JsString>, HostError> {
        self.host.get_directories_js(path)
    }

    /// A file found through a linked directory has its real name.
    fn realpath_js(&self, path: JsStr<'_>) -> Result<Option<JsString>, HostError> {
        if let Some(real) = self.known.borrow().files.get(&self.to_path(path)) {
            return Ok(Some(JsString::from(real.as_str())));
        }
        self.host.realpath_js(path)
    }

    fn current_directory(&self) -> Result<PathBuf, HostError> {
        self.host.current_directory()
    }

    fn use_case_sensitive_file_names(&self) -> bool {
        self.case_sensitive
    }

    fn read_file(&self, path: &Path) -> Result<Option<Vec<u8>>, HostError> {
        self.host.read_file(path)
    }

    fn file_exists(&self, path: &Path) -> Result<bool, HostError> {
        match path.to_str() {
            Some(path) => self.file_exists_js(JsStr::from_str(path)),
            None => self.host.file_exists(path),
        }
    }

    fn directory_exists(&self, path: &Path) -> Result<bool, HostError> {
        match path.to_str() {
            Some(path) => self.directory_exists_js(JsStr::from_str(path)),
            None => self.host.directory_exists(path),
        }
    }

    fn read_directory(&self, path: &Path) -> Result<Vec<PathBuf>, HostError> {
        self.host.read_directory(path)
    }

    fn get_directories(&self, path: &Path) -> Result<Vec<PathBuf>, HostError> {
        self.host.get_directories(path)
    }

    fn realpath(&self, path: &Path) -> Result<Option<PathBuf>, HostError> {
        match path.to_str() {
            Some(path) => Ok(self
                .realpath_js(JsStr::from_str(path))?
                .map(|real| PathBuf::from(real.to_string_lossy().into_owned()))),
            None => self.host.realpath(path),
        }
    }
}

/// tsgo `tspath.ToPath`.
fn to_path(path: JsStr<'_>, current_directory: JsStr<'_>, case_sensitive: bool) -> String {
    file_name_key(
        normalized_absolute_path(path, current_directory).as_js(),
        case_sensitive,
    )
    .to_string_lossy()
    .into_owned()
}

/// tsgo `Path.ContainsPath`.
fn contains_path(parent: &str, child: &str) -> bool {
    !parent.is_empty()
        && (parent == child
            || child.len() > parent.len()
                && child.starts_with(parent)
                && (parent.ends_with('/') || child.as_bytes()[parent.len()] == b'/'))
}

fn with_trailing_separator(mut path: String) -> String {
    if !path.ends_with('/') {
        path.push('/');
    }
    path
}

/// tsgo `tspath.ContainsIgnoredPath`.
fn contains_ignored_path(path: JsStr<'_>) -> bool {
    ["/node_modules/.", "/.git", ".#"]
        .iter()
        .any(|pattern| path.contains(pattern))
}

/// tsgo `module.ParseNodeModuleFromPath(path, isFolder: true)`: the package
/// directory after the last `node_modules`, with its scope.
fn parse_node_module_from_path(path: JsStr<'_>) -> Option<String> {
    let path = normalize_path(path).to_string_lossy().into_owned();
    let index = path.rfind("/node_modules/")?;
    let after = index + "/node_modules/".len();
    let mut end = next_directory_separator(&path, after);
    if path.as_bytes().get(after) == Some(&b'@') {
        end = next_directory_separator(&path, end);
    }
    Some(path[..end].to_owned())
}

/// tsgo `moveToNextDirectorySeparatorIfAvailable` for a folder.
fn next_directory_separator(path: &str, previous: usize) -> usize {
    let offset = previous + 1;
    match path.get(offset..).and_then(|rest| rest.find('/')) {
        Some(next) => next + offset,
        None => path.len(),
    }
}
