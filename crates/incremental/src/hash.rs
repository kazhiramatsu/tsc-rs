//! tsgo `incremental.ComputeHash`: the XXH3-128 digest of a text as 32 hex
//! digits (the big-endian bytes of the 128-bit value).

/// tsgo snapshot.go `ComputeHash(text, hashWithText=false)`.
pub fn compute_hash(text: &[u8]) -> String {
    format!("{:032x}", xxhash_rust::xxh3::xxh3_128(text))
}

#[cfg(test)]
mod tests {
    use super::compute_hash;

    #[test]
    fn matches_the_digests_tsgo_wrote() {
        // `src/b.ts` of the errors fixture and `src/other.ts` of the
        // dtserror fixture: tsgo's fileInfos versions.
        assert_eq!(
            compute_hash(b"export const y = 1;\n"),
            "f4c10273d8d214abf45efaa293b7954f"
        );
        assert_eq!(
            compute_hash(b"export function add(a: number, b: number): number { return a + b; }\n"),
            "d948eb9f758e7b70ae5eb35555896ac2"
        );
        assert_eq!(compute_hash(b""), "99aa06d3014798d86001c324468d497f");
    }
}
