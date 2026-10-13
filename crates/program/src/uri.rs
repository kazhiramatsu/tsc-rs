//! tsgo's document URIs as file names (`lsproto.DocumentUri.Path`,
//! lsp/lsproto/lsp.go, with tspath/dynamic.go, at aa8149273). A `file:` URI
//! is its decoded path. Any other URI is a dynamic name: `^/~ts-uri~/`, the
//! scheme, the authority (`ts-nul-authority` without one) and the path, its
//! escapes kept, with each segment a file name cannot hold (empty, `.`,
//! `..`, a backslash, an escape's own prefix) written as
//! `~ts-uri-escape~<hex>~<extension>`, and a query or fragment folded into
//! the last segment.

use tsc_diagnostics::JsStr;

use crate::DYNAMIC_URI_FILE_NAME_PREFIX;

const PATH_SEGMENT_ESCAPE_PREFIX: &str = "~ts-uri-escape~";
const MODULE_SPECIFIER_ESCAPE_PREFIX: &str = "~ts-uri-spec~";
const NO_PATH_ESCAPE_PREFIX: &str = "~ts-uri-no-path~";
const NUL_AUTHORITY: &str = "ts-nul-authority";

/// tsgo `bundled.IsBundled`: a file of the embedded standard library.
fn is_bundled(name: &str) -> bool {
    name.starts_with("bundled:///")
}

/// tsgo `DocumentUri.Path`: a `file:` URI's path, percent-decoded
/// (`//host/path` for one with a host, without the slash before a drive
/// letter), and any other URI as its dynamic name.
pub fn uri_to_file_name(uri: &str) -> Result<String, String> {
    if is_bundled(uri) {
        return Ok(uri.to_owned());
    }
    if let Some(rest) = uri.strip_prefix("file://") {
        // url.Parse: the authority up to the path, the path up to a query
        // or fragment.
        let rest = rest.split(['?', '#']).next().unwrap_or_default();
        let (host, path) = match rest.find('/') {
            Some(index) => rest.split_at(index),
            None => (rest, ""),
        };
        let invalid = || format!("invalid file URI: {uri}");
        let host = percent_decode(host).ok_or_else(invalid)?;
        let path = percent_decode(path).ok_or_else(invalid)?;
        let path = if !host.is_empty() {
            format!("//{host}{path}")
        } else {
            // fixWindowsURIPath.
            match path.strip_prefix('/') {
                Some(rest)
                    if rest.len() >= 2
                        && rest.as_bytes()[0].is_ascii_alphabetic()
                        && rest.as_bytes()[1] == b':' =>
                {
                    rest.to_owned()
                }
                _ => path,
            }
        };
        return Ok(rooted_path_from_absolute(&path));
    }
    let Some((scheme, path)) = uri.split_once(':') else {
        return Err(format!("invalid URI: {uri}"));
    };
    let (mut path, suffix) = match path.find(['?', '#']) {
        Some(index) => path.split_at(index),
        None => (path, ""),
    };
    let mut authority = NUL_AUTHORITY;
    let mut has_authority = false;
    let mut has_path = true;
    if let Some(rest) = path.strip_prefix("//") {
        has_authority = true;
        match rest.split_once('/') {
            Some((name, rest)) => (authority, path) = (name, rest),
            None => {
                (authority, path) = (rest, "");
                has_path = false;
            }
        }
    }
    let encoded_authority = match (has_authority, authority) {
        (false, _) => authority.to_owned(),
        (true, NUL_AUTHORITY) => force_encode_segment(authority, false),
        (true, _) => encode_path(authority),
    };
    let encoded_path = if has_path {
        encode_path_with_suffix(path, suffix)
    } else {
        format!("{NO_PATH_ESCAPE_PREFIX}{}~", hex(suffix.as_bytes()))
    };
    Ok(format!(
        "{DYNAMIC_URI_FILE_NAME_PREFIX}{scheme}/{encoded_authority}/{encoded_path}"
    ))
}

/// tsgo `RootedPathFromAbsolute`: normalized, a root alone ending with a
/// separator.
pub fn rooted_path_from_absolute(path: &str) -> String {
    let mut normalized = crate::get_normalized_absolute_path(JsStr::from_str(path), "".into())
        .to_string_lossy()
        .into_owned();
    let root_only = crate::path_root_parts(JsStr::from_str(&normalized))
        .is_some_and(|(_, rest)| rest.is_empty());
    if root_only && !normalized.ends_with('/') {
        normalized.push('/');
    }
    normalized
}

/// tsgo `EncodeDynamicURIPath`.
fn encode_path(path: &str) -> String {
    encode_path_root_aware(path, true)
}

/// tsgo `EncodeDynamicURIPathWithSuffix`: the suffix folded into the last
/// segment, which is always escaped then.
fn encode_path_with_suffix(path: &str, suffix: &str) -> String {
    if suffix.is_empty() {
        return encode_path(path);
    }
    let (before, segment) = match path.rfind('/') {
        Some(slash) => (
            format!("{}/", encode_path_root_aware(&path[..slash], false)),
            &path[slash + 1..],
        ),
        None => (String::new(), path),
    };
    let (base, extension) = split_extension(segment);
    let payload = format!("{base}\0{suffix}");
    format!(
        "{before}{PATH_SEGMENT_ESCAPE_PREFIX}{}~{extension}",
        hex(payload.as_bytes())
    )
}

/// tsgo `encodeDynamicURIPathRootAware`: a path that is rooted after its
/// segments' escapes has its first segment escaped too.
fn encode_path_root_aware(path: &str, preserve_final_extension: bool) -> String {
    let encoded = encode_segments(path, preserve_final_extension);
    if crate::path_root_parts(JsStr::from_str(&encoded)).is_none() {
        return encoded;
    }
    match encoded.split_once('/') {
        None => force_encode_segment(&encoded, preserve_final_extension),
        Some((first, rest)) => format!("{}/{rest}", force_encode_segment(first, false)),
    }
}

/// tsgo `encodeDynamicURIPath`.
fn encode_segments(path: &str, preserve_final_extension: bool) -> String {
    if !path.split('/').any(segment_needs_encoding) {
        return path.to_owned();
    }
    let segments = path.split('/').collect::<Vec<_>>();
    let last = segments.len() - 1;
    segments
        .iter()
        .enumerate()
        .map(|(index, segment)| {
            if segment_needs_encoding(segment) {
                force_encode_segment(segment, preserve_final_extension && index == last)
            } else {
                (*segment).to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// tsgo `dynamicURIPathSegmentNeedsEncoding`.
fn segment_needs_encoding(segment: &str) -> bool {
    segment.is_empty()
        || segment == "."
        || segment == ".."
        || segment.starts_with(PATH_SEGMENT_ESCAPE_PREFIX)
        || segment.starts_with(MODULE_SPECIFIER_ESCAPE_PREFIX)
        || segment.starts_with(NO_PATH_ESCAPE_PREFIX)
        || segment.contains('\\')
}

/// tsgo `forceEncodeDynamicURIPathSegment`.
fn force_encode_segment(segment: &str, preserve_extension: bool) -> String {
    let (base, extension) = if preserve_extension && segment != "." && segment != ".." {
        split_extension(segment)
    } else {
        (segment, "")
    };
    format!(
        "{PATH_SEGMENT_ESCAPE_PREFIX}{}~{extension}",
        hex(base.as_bytes())
    )
}

/// tsgo `splitDynamicURIFileExtension`: a declaration file's extension
/// (`.d.ts`, `.d.cts`, `.d.mts`, `.d.<x>.ts`), else the text from the last
/// dot of the name after any backslash.
fn split_extension(segment: &str) -> (&str, &str) {
    let base_name = &segment[segment.rfind('\\').map_or(0, |index| index + 1)..];
    let base_name = &base_name[base_name.rfind('/').map_or(0, |index| index + 1)..];
    let declaration = [".d.ts", ".d.cts", ".d.mts"]
        .into_iter()
        .find(|extension| base_name.ends_with(extension))
        .or_else(|| {
            base_name
                .ends_with(".ts")
                .then(|| base_name.find(".d.").map(|index| &base_name[index..]))
                .flatten()
        });
    let extension = declaration
        .or_else(|| base_name.rfind('.').map(|index| &base_name[index..]))
        .unwrap_or_default();
    (&segment[..segment.len() - extension.len()], extension)
}

/// Go's `hex.EncodeToString`.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The bytes `%XX` escapes stand for, as UTF-8; nothing for a malformed
/// escape or text.
fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = text.get(index + 1..index + 3)?;
            decoded.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

#[cfg(test)]
mod tests {
    use super::uri_to_file_name;

    #[test]
    fn uris_take_tsgos_file_names() {
        // tsgo aa8149273's `TestDocumentURIToFileName` and
        // `TestNonFileDocumentURIRoundTripsThroughNormalizedFileName`
        // (ls/lsconv/converters_test.go), and the client test's documents.
        for (uri, expected) in [
            ("file:///path/to/file.ts", "/path/to/file.ts"),
            ("file://server/share/file.ts", "//server/share/file.ts"),
            ("file:///d%3A/work/tsgo932/lib/utils.ts", "d:/work/tsgo932/lib/utils.ts"),
            (
                "file:///d%3A/work/tsgo932/app/%28test%29/comp/comp-test.tsx",
                "d:/work/tsgo932/app/(test)/comp/comp-test.tsx",
            ),
            ("file:///path/to/file.ts#section", "/path/to/file.ts"),
            ("file:///c:/test/me", "c:/test/me"),
            ("file://shares/files/c%23/p.cs", "//shares/files/c#/p.cs"),
            ("file:///c:/test %25/path", "c:/test %/path"),
            ("file:///_:/path", "/_:/path"),
            ("file:///users/me/c%23-projects/", "/users/me/c#-projects"),
            ("file:///a/../b.ts", "/b.ts"),
            (
                "file://localhost/c%24/GitDevelopment/express",
                "//localhost/c$/GitDevelopment/express",
            ),
            ("file:///c%3A/test%20with%20%2525/c%23code", "c:/test with %25/c#code"),
            ("file:///workspace/file%20name.ts", "/workspace/file name.ts"),
            ("untitled:Untitled-1", "^/~ts-uri~/untitled/ts-nul-authority/Untitled-1"),
            (
                "untitled:Untitled-1#fragment",
                "^/~ts-uri~/untitled/ts-nul-authority/~ts-uri-escape~556e7469746c65642d310023667261676d656e74~",
            ),
            (
                "untitled:c:/Users/jrieken/Code/abc.txt",
                "^/~ts-uri~/untitled/ts-nul-authority/~ts-uri-escape~633a~/Users/jrieken/Code/abc.txt",
            ),
            (
                "untitled:C:/Users/jrieken/Code/abc.txt",
                "^/~ts-uri~/untitled/ts-nul-authority/~ts-uri-escape~433a~/Users/jrieken/Code/abc.txt",
            ),
            (
                "untitled://wsl%2Bubuntu/home/jabaile/work/TypeScript/newfile.ts",
                "^/~ts-uri~/untitled/wsl%2Bubuntu/home/jabaile/work/TypeScript/newfile.ts",
            ),
            (
                r"custom:folder/../~ts-uri~/café\file.ts",
                "^/~ts-uri~/custom/ts-nul-authority/folder/~ts-uri-escape~2e2e~/~ts-uri~/~ts-uri-escape~636166c3a95c66696c65~.ts",
            ),
            ("custom:.git/file.ts", "^/~ts-uri~/custom/ts-nul-authority/.git/file.ts"),
            (
                "custom:~ts-uri-escape~dir.js/file.ts?x=1",
                "^/~ts-uri~/custom/ts-nul-authority/~ts-uri-escape~7e74732d7572692d6573636170657e6469722e6a73~/~ts-uri-escape~66696c65003f783d31~.ts",
            ),
            (
                "vscode-remote://ssh-remote+host/workspace/src/remote%20name.ts",
                "^/~ts-uri~/vscode-remote/ssh-remote+host/workspace/src/remote%20name.ts",
            ),
            (
                "vscode-notebook-cell:/workspace/notebook.ipynb/cell%20name.ts",
                "^/~ts-uri~/vscode-notebook-cell/ts-nul-authority/~ts-uri-escape~~/workspace/notebook.ipynb/cell%20name.ts",
            ),
        ] {
            assert_eq!(uri_to_file_name(uri).unwrap(), expected, "{uri}");
        }
    }
}
