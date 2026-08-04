use std::cmp::Ordering;

/// Compare JavaScript strings in the order used by relational comparison and
/// `Array.prototype.sort`: lexicographically by UTF-16 code unit.
///
/// Rust's `str::cmp` compares UTF-8 bytes instead. The two orders differ when
/// an astral character is compared with a BMP character above its high
/// surrogate, so host directory enumeration cannot use the native Rust order.
pub(crate) fn compare_utf16(left: &str, right: &str) -> Ordering {
    left.encode_utf16().cmp(right.encode_utf16())
}
