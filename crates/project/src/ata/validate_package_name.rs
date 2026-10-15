//! tsgo `validatepackagename.go`: npm's rules for the package names the
//! typings installer asks npm for.

/// tsgo `NameValidationResult`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NameValidationResult {
    NameOk,
    EmptyName,
    NameTooLong,
    NameStartsWithDot,
    NameStartsWithUnderscore,
    NameContainsNonUriSafeCharacters,
}

/// tsgo `maxPackageNameLength`.
const MAX_PACKAGE_NAME_LENGTH: usize = 214;

/// tsgo `ValidatePackageName`: the result, the name it is about (a scoped
/// package's scope or name) and whether that is the scope.
pub fn validate_package_name(package_name: &str) -> (NameValidationResult, String, bool) {
    validate_package_name_worker(package_name, true)
}

fn validate_package_name_worker(
    package_name: &str,
    support_scoped_package: bool,
) -> (NameValidationResult, String, bool) {
    if package_name.is_empty() {
        return (NameValidationResult::EmptyName, String::new(), false);
    }
    if package_name.len() > MAX_PACKAGE_NAME_LENGTH {
        return (NameValidationResult::NameTooLong, String::new(), false);
    }
    if package_name.starts_with('.') {
        return (
            NameValidationResult::NameStartsWithDot,
            String::new(),
            false,
        );
    }
    if package_name.starts_with('_') {
        return (
            NameValidationResult::NameStartsWithUnderscore,
            String::new(),
            false,
        );
    }
    // A scoped package: `@scope/name` with one `/`.
    if support_scoped_package {
        if let Some((scope, name)) = package_name
            .strip_prefix('@')
            .and_then(|without_scope| without_scope.split_once('/'))
        {
            if !scope.is_empty() && !name.is_empty() && !name.contains('/') {
                let (scope_result, _, _) = validate_package_name_worker(scope, false);
                if scope_result != NameValidationResult::NameOk {
                    return (scope_result, scope.to_owned(), true);
                }
                let (name_result, _, _) = validate_package_name_worker(name, false);
                if name_result != NameValidationResult::NameOk {
                    return (name_result, name.to_owned(), false);
                }
                return (NameValidationResult::NameOk, String::new(), false);
            }
        }
    }
    // Go's `url.QueryEscape` leaves only these as they are.
    if !package_name
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~'))
    {
        return (
            NameValidationResult::NameContainsNonUriSafeCharacters,
            String::new(),
            false,
        );
    }
    (NameValidationResult::NameOk, String::new(), false)
}

/// tsgo `renderPackageNameValidationFailure`.
pub(super) fn render_package_name_validation_failure(
    typing: &str,
    result: NameValidationResult,
    name: &str,
    is_scope_name: bool,
) -> String {
    let kind = if is_scope_name { "Scope" } else { "Package" };
    let name = if name.is_empty() { typing } else { name };
    match result {
        NameValidationResult::EmptyName => {
            format!("'{typing}':: {kind} name '{name}' cannot be empty")
        }
        NameValidationResult::NameTooLong => format!(
            "'{typing}':: {kind} name '{name}' should be less than {MAX_PACKAGE_NAME_LENGTH} characters"
        ),
        NameValidationResult::NameStartsWithDot => {
            format!("'{typing}':: {kind} name '{name}' cannot start with '.'")
        }
        NameValidationResult::NameStartsWithUnderscore => {
            format!("'{typing}':: {kind} name '{name}' cannot start with '_'")
        }
        NameValidationResult::NameContainsNonUriSafeCharacters => {
            format!("'{typing}':: {kind} name '{name}' contains non URI safe characters")
        }
        NameValidationResult::NameOk => unreachable!("Unexpected Ok result"),
    }
}
