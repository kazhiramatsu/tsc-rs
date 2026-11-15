use std::cmp::Ordering;

/// Native directory entries follow Go's bytewise name ordering. The
/// filesystem's case profile affects identity, not this display-name order.
pub(crate) fn compare_file_names(left: &str, right: &str) -> Ordering {
    left.cmp(right)
}
