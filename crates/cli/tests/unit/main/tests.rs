use super::*;

#[test]
fn the_command_line_build_puts_source_texts_in_the_text_heap() {
    // The binary's mimalloc is v3 and makes the text heap: a source text
    // past 8 KiB does not fall back to the default heap.
    let source = "export const value = 1;\n".repeat(1_000);
    let text = tsc_store::SharedText::new(&source);
    assert!(text.in_text_heap());
    // runGC reaches the text heap and leaves live texts readable.
    collect_memory();
    assert_eq!(text.as_str(), source);
}
