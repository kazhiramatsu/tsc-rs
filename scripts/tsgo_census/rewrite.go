// Rewrite only the private upstream copy. No regular expressions parse Go:
// go/parser resolves declarations and byte offsets, and go/format validates edits.
package main

import (
	"bufio"
	"encoding/json"
	"flag"
	"fmt"
	"go/ast"
	"go/format"
	"go/parser"
	"go/token"
	"os"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
)

const censusImport = "github.com/microsoft/TypeScript/tsc/internal/census"

type entry struct {
	Source string `json:"source"`
	Name   string `json:"name"`
}

type edit struct {
	start, end int
	text       string
}

func must(err error) {
	if err != nil {
		fmt.Fprintln(os.Stderr, "census rewrite:", err)
		os.Exit(1)
	}
}

func writeGo(path string, data []byte) {
	formatted, err := format.Source(data)
	must(err)
	must(os.WriteFile(path, formatted, 0o644))
}

func receiver(fn *ast.FuncDecl) string {
	if fn.Recv == nil {
		return ""
	}
	t := fn.Recv.List[0].Type
	if ptr, ok := t.(*ast.StarExpr); ok {
		t = ptr.X
	}
	id, ok := t.(*ast.Ident)
	if !ok {
		return "<unsupported receiver>"
	}
	return id.Name + "."
}

func apply(path string, data []byte, edits []edit) {
	sort.Slice(edits, func(i, j int) bool { return edits[i].start > edits[j].start })
	for _, e := range edits {
		data = append(append(append([]byte{}, data[:e.start]...), e.text...), data[e.end:]...)
	}
	writeGo(path, data)
}

func main() {
	root := flag.String("root", "", "private copy's tsc directory")
	manifest := flag.String("manifest", "", "functions.tsv")
	packageSource := flag.String("package-source", "", "census.go")
	report := flag.String("report", "", "rewrite report JSON")
	flag.Parse()
	if *root == "" || *manifest == "" || *packageSource == "" || *report == "" {
		must(fmt.Errorf("all four flags are required"))
	}
	file, err := os.Open(*manifest)
	must(err)
	scanner := bufio.NewScanner(file)
	groups := map[string][]entry{}
	var entries []entry
	seen := map[string]bool{}
	for scanner.Scan() {
		line := scanner.Text()
		if strings.TrimSpace(line) == "" || strings.HasPrefix(line, "#") {
			continue
		}
		cols := strings.Split(line, "\t")
		if len(cols) != 3 {
			must(fmt.Errorf("expected three TSV columns: %s", line))
		}
		path, symbol, ok := strings.Cut(cols[0], ":")
		if !ok || filepath.IsAbs(path) || filepath.Clean(path) != path || strings.HasPrefix(path, "..") {
			must(fmt.Errorf("invalid source location: %s", cols[0]))
		}
		parts := strings.Split(symbol, ".")
		name := parts[len(parts)-1]
		if !token.IsIdentifier(name) || name == "" {
			must(fmt.Errorf("invalid function name: %s", symbol))
		}
		name = strings.ToLower(name[:1]) + name[1:]
		if seen[name] {
			must(fmt.Errorf("duplicate JSON key: %s", name))
		}
		seen[name] = true
		e := entry{cols[0], name}
		groups[path] = append(groups[path], e)
		entries = append(entries, e)
	}
	must(scanner.Err())
	must(file.Close())
	if len(entries) == 0 {
		must(fmt.Errorf("empty manifest"))
	}
	found := map[string]bool{}
	for relative, group := range groups {
		path := filepath.Join(*root, relative)
		// A copied symlink must never lead the rewriter outside the copy.
		resolved, err := filepath.EvalSymlinks(path)
		if os.IsNotExist(err) {
			continue
		}
		must(err)
		rel, err := filepath.Rel(*root, resolved)
		must(err)
		if rel == ".." || strings.HasPrefix(rel, ".."+string(filepath.Separator)) {
			must(fmt.Errorf("source escapes private copy: %s", path))
		}
		data, err := os.ReadFile(path)
		must(err)
		fset := token.NewFileSet()
		parsed, err := parser.ParseFile(fset, path, data, 0)
		must(err)
		wanted := map[string]entry{}
		for _, e := range group {
			_, symbol, _ := strings.Cut(e.Source, ":")
			wanted[symbol] = e
		}
		var edits []edit
		for _, decl := range parsed.Decls {
			fn, ok := decl.(*ast.FuncDecl)
			if !ok || fn.Body == nil {
				continue
			}
			e, ok := wanted[receiver(fn)+fn.Name.Name]
			if !ok {
				continue
			}
			if found[e.Source] {
				must(fmt.Errorf("ambiguous declaration: %s", e.Source))
			}
			found[e.Source] = true
			offset := fset.Position(fn.Body.Lbrace).Offset + 1
			enum := strings.ToUpper(e.Name[:1]) + e.Name[1:]
			edits = append(edits, edit{offset, offset, "\n\tcensus.Enter(census." + enum + ")\n"})
		}
		if len(edits) > 0 {
			offset := fset.Position(parsed.Name.End()).Offset
			edits = append(edits, edit{offset, offset, "\n\nimport " + strconv.Quote(censusImport) + "\n"})
			apply(path, data, edits)
		}
	}
	var instrumented, missing []entry
	var generated strings.Builder
	generated.WriteString("// Code generated from functions.tsv; DO NOT EDIT.\npackage census\ntype Function int\nconst (\n")
	for _, e := range entries {
		if !found[e.Source] {
			missing = append(missing, e)
			fmt.Fprintln(os.Stderr, "census: function not found (skipped):", e.Source)
			continue
		}
		if len(instrumented) == 0 {
			fmt.Fprintf(&generated, "%s Function = iota\n", strings.ToUpper(e.Name[:1])+e.Name[1:])
		} else {
			fmt.Fprintln(&generated, strings.ToUpper(e.Name[:1])+e.Name[1:])
		}
		instrumented = append(instrumented, e)
	}
	if len(instrumented) == 0 {
		must(fmt.Errorf("no declarations matched"))
	}
	generated.WriteString("functionCount\n)\nvar functionNames = [...]string{\n")
	for _, e := range instrumented {
		fmt.Fprintf(&generated, "%q,\n", e.Name)
	}
	generated.WriteString("}\n")
	destination := filepath.Join(*root, "internal/census")
	must(os.Mkdir(destination, 0o755)) // A fresh copy must not already contain census.
	data, err := os.ReadFile(*packageSource)
	must(err)
	writeGo(filepath.Join(destination, "census.go"), data)
	writeGo(filepath.Join(destination, "functions.go"), []byte(generated.String()))
	// os.Exit skips defers. Replace its argument evaluation with an explicit dump
	// after runMain returns, retaining that exact exit status and all output.
	path := filepath.Join(*root, "cmd/tsc/main.go")
	data, err = os.ReadFile(path)
	must(err)
	fset := token.NewFileSet()
	parsed, err := parser.ParseFile(fset, path, data, 0)
	must(err)
	var edits []edit
	for _, decl := range parsed.Decls {
		fn, ok := decl.(*ast.FuncDecl)
		if !ok || fn.Name.Name != "main" || fn.Recv != nil {
			continue
		}
		body := strings.TrimSpace(string(data[fset.Position(fn.Body.Lbrace).Offset+1 : fset.Position(fn.Body.Rbrace).Offset]))
		if body != "os.Exit(runMain())" {
			must(fmt.Errorf("unexpected cmd/tsc main body: %s", body))
		}
		edits = append(edits, edit{fset.Position(fn.Body.Lbrace).Offset + 1, fset.Position(fn.Body.Rbrace).Offset,
			"\nstatus := runMain()\n_ = census.Write()\nos.Exit(status)\n"})
	}
	if len(edits) != 1 {
		must(fmt.Errorf("expected exactly one main function"))
	}
	offset := fset.Position(parsed.Name.End()).Offset
	edits = append(edits, edit{offset, offset, "\n\nimport " + strconv.Quote(censusImport) + "\n"})
	apply(path, data, edits)
	data, err = json.MarshalIndent(map[string]any{"instrumented": instrumented, "missing": missing}, "", "  ")
	must(err)
	must(os.WriteFile(*report, append(data, '\n'), 0o644))
	fmt.Printf("census: instrumented %d functions; skipped %d\n", len(instrumented), len(missing))
}
