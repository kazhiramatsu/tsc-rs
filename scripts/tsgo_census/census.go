// Package census counts checker entries in a single-threaded compiler run.
// Enter deliberately uses a plain, non-atomic increment: callers MUST use
// --singleThreaded. The instrumented compiler is not safe for parallel checking.
package census

import (
	"encoding/json"
	"os"
)

// Function and functionNames are generated from functions.tsv by rewrite.go.
var counters = make([]uint64, int(functionCount))

func Enter(i Function) {
	counters[i]++
}

// Write is called explicitly before os.Exit, after compilation has returned.
// The caller ignores its error to preserve the compiler's output and exit code;
// the Python runner separately verifies that a fresh, valid dump was written.
func Write() error {
	path := os.Getenv("TSGO_CENSUS")
	if path == "" {
		return nil
	}
	counts := make(map[string]uint64, len(functionNames))
	for i, name := range functionNames {
		counts[name] = counters[i]
	}
	data, err := json.MarshalIndent(counts, "", "  ")
	if err != nil {
		return err
	}
	return os.WriteFile(path, append(data, '\n'), 0o644)
}
