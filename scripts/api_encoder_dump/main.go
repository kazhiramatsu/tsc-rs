// tsrsencdump writes TypeScript 7.1's API encoding of source files, for
// tsc-rs's encoder tests (scripts/api_encoder_dump.py copies it into the
// pinned checkout as cmd/tsrsencdump, builds it and removes it).
//
// usage: tsrsencdump <list file> <out dir>
//
// Each line of the list is "<index>\t<file name>\t<path on disk>". The file
// is parsed as tsgo's API session parses it (the file name as its name and
// path, the script kind of its extension, the default external-module
// options), its content hash is set as the session's file system sets it,
// and encoder.EncodeSourceFile's bytes are written to <out dir>/<index>.bin.
package main

import (
	"bufio"
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/microsoft/TypeScript/tsc/internal/api/encoder"
	"github.com/microsoft/TypeScript/tsc/internal/ast"
	"github.com/microsoft/TypeScript/tsc/internal/core"
	"github.com/microsoft/TypeScript/tsc/internal/parser"
	"github.com/microsoft/TypeScript/tsc/internal/tspath"
	"github.com/zeebo/xxh3"
)

func main() {
	if len(os.Args) != 3 {
		fmt.Fprintln(os.Stderr, "usage: tsrsencdump <list file> <out dir>")
		os.Exit(2)
	}
	list, err := os.Open(os.Args[1])
	if err != nil {
		panic(err)
	}
	defer list.Close()
	outDir := os.Args[2]
	scanner := bufio.NewScanner(list)
	scanner.Buffer(make([]byte, 1<<20), 1<<20)
	count := 0
	for scanner.Scan() {
		fields := strings.Split(scanner.Text(), "\t")
		if len(fields) != 3 {
			panic("bad list line: " + scanner.Text())
		}
		index, fileName, diskPath := fields[0], fields[1], fields[2]
		text, err := os.ReadFile(diskPath)
		if err != nil {
			panic(err)
		}
		sourceText := string(text)
		rooted := tspath.ToRootedFilePath(fileName, "/")
		sourceFile := parser.ParseSourceFile(ast.SourceFileParseOptions{
			FileName: rooted,
			PathKey:  tspath.CaseSensitive.PathKey(rooted.AsPath()),
		}, sourceText, core.GetScriptKindFromFileName(rooted))
		sourceFile.Hash = xxh3.HashString128(sourceText)
		data, _, err := encoder.EncodeSourceFile(sourceFile)
		if err != nil {
			panic(err)
		}
		if err := os.WriteFile(filepath.Join(outDir, index+".bin"), data, 0o644); err != nil {
			panic(err)
		}
		count++
	}
	if err := scanner.Err(); err != nil {
		panic(err)
	}
	fmt.Println("encoded", count)
}
