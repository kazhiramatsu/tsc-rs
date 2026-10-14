// Command tsrsunicodetables writes the Unicode tables of tsc-rs's organize
// imports comparers and auto-import index (crates/ls/src/lsutil/unicode_tables.rs) from the Go
// packages tsgo's comparers read: unicode and golang.org/x/text's norm.
//
// usage: tsrsunicodetables <output file>
package main

import (
	"fmt"
	"os"
	"strings"
	"unicode"

	"golang.org/x/text/unicode/norm"
)

func ranges(sb *strings.Builder, doc string, name string, table *unicode.RangeTable) {
	fmt.Fprintf(sb, "/// %s: ranges with a stride.\npub(crate) static %s: &[(u32, u32, u32)] = &[\n", doc, name)
	for _, r := range table.R16 {
		fmt.Fprintf(sb, "    (0x%04X, 0x%04X, %d),\n", r.Lo, r.Hi, r.Stride)
	}
	for _, r := range table.R32 {
		fmt.Fprintf(sb, "    (0x%04X, 0x%04X, %d),\n", r.Lo, r.Hi, r.Stride)
	}
	sb.WriteString("];\n\n")
}

func main() {
	if len(os.Args) != 2 {
		fmt.Fprintln(os.Stderr, "usage: tsrsunicodetables <output file>")
		os.Exit(2)
	}
	var sb strings.Builder
	sb.WriteString("//! Generated from Go's `unicode` and `golang.org/x/text/unicode/norm`\n")
	fmt.Fprintf(&sb, "//! (Unicode %s): the tables tsgo's organize imports comparers and auto-import\n//! index read.\n", unicode.Version)
	sb.WriteString("//! Regenerate with scripts/generate_unicode_tables.py.\n\n")
	ranges(&sb, "Go `unicode.Lu`", "UPPERCASE_LETTER", unicode.Lu)
	ranges(&sb, "Go `unicode.Ll`", "LOWERCASE_LETTER", unicode.Ll)
	ranges(&sb, "Go `unicode.Mn`", "NONSPACING_MARK", unicode.Mn)
	sb.WriteString("/// Go `unicode.ToLower` of each character it changes.\npub(crate) static TO_LOWER: &[(u32, u32)] = &[\n")
	for r := rune(0); r <= unicode.MaxRune; r++ {
		if l := unicode.ToLower(r); l != r {
			fmt.Fprintf(&sb, "    (0x%04X, 0x%04X),\n", r, l)
		}
	}
	sb.WriteString("];\n\n/// Go `unicode.ToUpper` of each character it changes.\npub(crate) static TO_UPPER: &[(u32, u32)] = &[\n")
	for r := rune(0); r <= unicode.MaxRune; r++ {
		if u := unicode.ToUpper(r); u != r {
			fmt.Fprintf(&sb, "    (0x%04X, 0x%04X),\n", r, u)
		}
	}
	sb.WriteString("];\n\n/// The canonical decomposition (Go `norm.NFD`) of each character that has\n/// one, Hangul syllables aside.\npub(crate) static CANONICAL_DECOMPOSITION: &[(u32, &str)] = &[\n")
	for r := rune(0); r <= unicode.MaxRune; r++ {
		if r >= 0xD800 && r <= 0xDFFF {
			continue
		}
		if r >= 0xAC00 && r <= 0xD7A3 {
			continue
		}
		s := string(r)
		d := norm.NFD.String(s)
		if d != s {
			var escaped strings.Builder
			for _, c := range d {
				fmt.Fprintf(&escaped, "\\u{%X}", c)
			}
			fmt.Fprintf(&sb, "    (0x%04X, \"%s\"),\n", r, escaped.String())
		}
	}
	sb.WriteString("];\n")
	if err := os.WriteFile(os.Args[1], []byte(sb.String()), 0o644); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
