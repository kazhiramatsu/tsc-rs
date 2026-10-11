// The scenario recorder scripts/tsctests_scenarios.py adds to a copy of
// tsgo's internal/execute/tsctests package. The tests run as they do
// upstream (and still compare their baselines); each scenario's inputs and
// each edit's file operations are recorded and written as JSON for the
// tsc-rs runner to replay.

package tsctests

import (
	"encoding/base64"
	"encoding/json"
	"fmt"
	"io/fs"
	"os"
	"slices"
	"strings"
	"sync"
	"testing"
	"testing/fstest"
	"time"
	"unicode/utf8"

	"github.com/microsoft/TypeScript/tsc/internal/core"
	"github.com/microsoft/TypeScript/tsc/internal/testutil/baseline"
	"github.com/microsoft/TypeScript/tsc/internal/tspath"
)

const tsrsDumpVariable = "TSRS_TSCTESTS_DUMP"

// tsrsText is file text: UTF-8 as is, other bytes in base64.
type tsrsText struct {
	Text   *string `json:"text,omitempty"`
	Base64 *string `json:"base64,omitempty"`
}

func tsrsTextOf(content string) tsrsText {
	if utf8.ValidString(content) {
		return tsrsText{Text: &content}
	}
	encoded := base64.StdEncoding.EncodeToString([]byte(content))
	return tsrsText{Base64: &encoded}
}

type tsrsFile struct {
	Path    string  `json:"path"`
	Symlink *string `json:"symlink,omitempty"`
	tsrsText
}

// tsrsOp is one TestSys file operation of an edit: write, append, prepend,
// replace (the first occurrence), replaceAll, remove, rename, or touch (the
// modification time set to the clock's next reading).
type tsrsOp struct {
	Op   string  `json:"op"`
	Path string  `json:"path"`
	To   string  `json:"to,omitempty"`
	Old  *string `json:"old,omitempty"`
	New  *string `json:"new,omitempty"`
	tsrsText
}

type tsrsEdit struct {
	Caption      string   `json:"caption"`
	Args         []string `json:"args"`
	ExpectedDiff string   `json:"expectedDiff,omitempty"`
	Ops          []tsrsOp `json:"ops"`
	// The operations of the edit on the clean system of the incremental
	// correctness check, when they differ (an edit can test
	// `sys.forIncrementalCorrectness`).
	NonIncrementalOps *[]tsrsOp `json:"nonIncrementalOps,omitempty"`
}

type tsrsScenario struct {
	Baseline         string            `json:"baseline"`
	Test             string            `json:"test"`
	Scenario         string            `json:"scenario"`
	SubScenario      string            `json:"subScenario"`
	Cwd              string            `json:"cwd,omitempty"`
	Args             []string          `json:"args"`
	Env              map[string]string `json:"env,omitempty"`
	OutputIsTTY      *bool             `json:"outputIsTTY,omitempty"`
	IgnoreCase       bool              `json:"ignoreCase,omitempty"`
	WindowsStyleRoot string            `json:"windowsStyleRoot,omitempty"`
	Files            []tsrsFile        `json:"files"`
	Edits            []tsrsEdit        `json:"edits"`

	mu                sync.Mutex
	nonIncrementalOps map[int][]tsrsOp
}

type tsrsRecorder struct {
	mu    sync.Mutex
	ops   []tsrsOp
	depth int
}

var (
	tsrsScenarios   sync.Map // baseline path -> *tsrsScenario
	tsrsDuplicates  sync.Map // baseline path -> true
	tsrsRecorders   sync.Map // *TestSys -> *tsrsRecorder
	tsrsFsRecorders sync.Map // *testFs -> *tsrsRecorder
)

// tsrsRecord records an operation of an edit in progress on `s` unless an
// outer recorded operation is performing it; the returned function ends it.
func tsrsRecord(s *TestSys, op tsrsOp) func() {
	value, ok := tsrsRecorders.Load(s)
	if !ok {
		return func() {}
	}
	recorder := value.(*tsrsRecorder)
	recorder.mu.Lock()
	if recorder.depth == 0 {
		recorder.ops = append(recorder.ops, op)
	}
	recorder.depth++
	recorder.mu.Unlock()
	return func() {
		recorder.mu.Lock()
		recorder.depth--
		recorder.mu.Unlock()
	}
}

func tsrsString(value string) *string {
	return &value
}

// Chtimes records an edit's touch (an edit sets the time to the clock's
// next reading) and changes the time.
func (f *testFs) Chtimes(path tspath.RootedPath, aTime time.Time, mTime time.Time) error {
	if value, ok := tsrsFsRecorders.Load(f); ok {
		recorder := value.(*tsrsRecorder)
		recorder.mu.Lock()
		if recorder.depth == 0 {
			recorder.ops = append(recorder.ops, tsrsOp{Op: "touch", Path: string(path)})
		}
		recorder.mu.Unlock()
	}
	return f.FS.Chtimes(path, aTime, mTime)
}

func tsrsDumpBegin(t *testing.T, test *tscInput, scenario string) *tsrsScenario {
	if os.Getenv(tsrsDumpVariable) == "" {
		return nil
	}
	record := &tsrsScenario{
		Baseline:         test.getBaselineSubFolder() + "/" + scenario + "/" + strings.ReplaceAll(test.subScenario, " ", "-") + ".js",
		Test:             t.Name(),
		Scenario:         scenario,
		SubScenario:      test.subScenario,
		Cwd:              test.cwd,
		Args:             test.commandLineArgs,
		Env:              test.env,
		OutputIsTTY:      test.outputIsTTY,
		IgnoreCase:       test.ignoreCase,
		WindowsStyleRoot: test.windowsStyleRoot,
		Files:            []tsrsFile{},
		Edits:            []tsrsEdit{},
	}
	if record.Args == nil {
		record.Args = []string{}
	}
	for path, value := range test.files {
		file := tsrsFile{Path: path}
		switch value := value.(type) {
		case string:
			file.tsrsText = tsrsTextOf(value)
		case []byte:
			file.tsrsText = tsrsTextOf(string(value))
		case *fstest.MapFile:
			if value.Mode&fs.ModeSymlink != 0 {
				file.Symlink = tsrsString(string(value.Data))
			} else {
				file.tsrsText = tsrsTextOf(string(value.Data))
			}
		default:
			panic(fmt.Sprintf("tsrs dump: %s: unexpected file value %T", path, value))
		}
		record.Files = append(record.Files, file)
	}
	slices.SortFunc(record.Files, func(a, b tsrsFile) int { return strings.Compare(a.Path, b.Path) })
	return record
}

func tsrsDumpEdit(record *tsrsScenario, sys *TestSys, do *tscEdit) {
	if record == nil {
		if do.edit != nil {
			do.edit(sys)
		}
		return
	}
	edit := tsrsEdit{
		Caption:      do.caption,
		Args:         do.commandLineArgs,
		ExpectedDiff: do.expectedDiff,
		Ops:          []tsrsOp{},
	}
	if do.edit != nil {
		recorder := &tsrsRecorder{}
		tsrsRecorders.Store(sys, recorder)
		tsrsFsRecorders.Store(sys.fs, recorder)
		do.edit(sys)
		tsrsRecorders.Delete(sys)
		tsrsFsRecorders.Delete(sys.fs)
		edit.Ops = recorder.ops
	}
	record.Edits = append(record.Edits, edit)
}

// tsrsDumpNonIncrementalEdit applies edit `i` to the clean system of the
// check after edit `index`, recording it when it is that edit.
func tsrsDumpNonIncrementalEdit(record *tsrsScenario, index int, i int, sys *TestSys, do *tscEdit) {
	if do.edit == nil {
		return
	}
	if record == nil || i != index {
		do.edit(sys)
		return
	}
	recorder := &tsrsRecorder{}
	tsrsRecorders.Store(sys, recorder)
	tsrsFsRecorders.Store(sys.fs, recorder)
	do.edit(sys)
	tsrsRecorders.Delete(sys)
	tsrsFsRecorders.Delete(sys.fs)
	record.mu.Lock()
	if record.nonIncrementalOps == nil {
		record.nonIncrementalOps = map[int][]tsrsOp{}
	}
	record.nonIncrementalOps[index] = recorder.ops
	record.mu.Unlock()
}

func tsrsDumpEnd(record *tsrsScenario) {
	if record == nil {
		return
	}
	for index := range record.Edits {
		ops, ok := record.nonIncrementalOps[index]
		if !ok {
			continue
		}
		mine, _ := json.Marshal(record.Edits[index].Ops)
		if ops == nil {
			ops = []tsrsOp{}
		}
		theirs, _ := json.Marshal(ops)
		if string(mine) != string(theirs) {
			if ops == nil {
				ops = []tsrsOp{}
			}
			record.Edits[index].NonIncrementalOps = &ops
		}
	}
	if _, loaded := tsrsScenarios.LoadOrStore(record.Baseline, record); loaded {
		tsrsDuplicates.Store(record.Baseline, true)
	}
}

func tsrsWriteDump(path string) error {
	var duplicates []string
	tsrsDuplicates.Range(func(key, _ any) bool {
		duplicates = append(duplicates, key.(string))
		return true
	})
	if len(duplicates) > 0 {
		return fmt.Errorf("scenarios share a baseline: %v", duplicates)
	}
	var scenarios []*tsrsScenario
	tsrsScenarios.Range(func(_, value any) bool {
		scenarios = append(scenarios, value.(*tsrsScenario))
		return true
	})
	slices.SortFunc(scenarios, func(a, b *tsrsScenario) int { return strings.Compare(a.Baseline, b.Baseline) })
	file, err := os.Create(path)
	if err != nil {
		return err
	}
	defer file.Close()
	encoder := json.NewEncoder(file)
	encoder.SetEscapeHTML(false)
	encoder.SetIndent("", " ")
	return encoder.Encode(map[string]any{"scenarios": scenarios})
}

func TestMain(m *testing.M) {
	core.ApplyDebugStackLimit()
	defer baseline.Track()()
	code := m.Run()
	if path := os.Getenv(tsrsDumpVariable); path != "" && code == 0 {
		if err := tsrsWriteDump(path); err != nil {
			fmt.Fprintln(os.Stderr, "tsrs dump:", err)
			os.Exit(1)
		}
	}
}
