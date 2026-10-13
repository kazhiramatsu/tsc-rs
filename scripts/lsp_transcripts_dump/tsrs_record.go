// The transcript recorder scripts/lsp_transcripts.py adds to a copy of
// tsgo's internal/testutil/lsptestutil package, which a copy of the
// fourslash harness uses. The tests run as they do upstream; each harness's
// files and options, the messages both ways and the harness's file
// operations are recorded in order and written as JSON lines for the tsc-rs
// runner to replay.

package lsptestutil

import (
	"bytes"
	"encoding/base64"
	stdjson "encoding/json"
	"fmt"
	"io"
	"io/fs"
	"os"
	"strconv"
	"strings"
	"sync"
	"testing"
	"testing/fstest"
	"unicode/utf8"

	"github.com/microsoft/TypeScript/tsc/internal/json"
)

const tsrsVariable = "TSRS_LSP_TRANSCRIPTS"

// TsrsRecording is one harness's transcript.
type TsrsRecording struct {
	mu     sync.Mutex
	header map[string]any
	events []any
}

var (
	tsrsOutputMu sync.Mutex
	tsrsNames    = map[string]int{}
)

// tsrsText is file text: UTF-8 as is, other bytes in base64.
func tsrsText(content string) any {
	if utf8.ValidString(content) {
		return content
	}
	return map[string]string{"base64": base64.StdEncoding.EncodeToString([]byte(content))}
}

// TsrsBegin starts the recording of a harness: its files (text, or a
// symbolic link's target), the file names' case sensitivity and the inferred
// projects' compiler options.
func TsrsBegin(t *testing.T, files map[string]any, caseSensitive bool, options any) *TsrsRecording {
	if os.Getenv(tsrsVariable) == "" {
		return nil
	}
	encoded := map[string]any{}
	for path, file := range files {
		switch file := file.(type) {
		case string:
			encoded[path] = tsrsText(file)
		case *fstest.MapFile:
			if file.Mode&fs.ModeSymlink == 0 {
				encoded[path] = tsrsText(string(file.Data))
			} else {
				encoded[path] = map[string]string{"symlink": string(file.Data)}
			}
		default:
			panic(fmt.Sprintf("tsrs: unknown file %T at %s", file, path))
		}
	}
	inferred, err := json.Marshal(options)
	if err != nil {
		panic(err)
	}
	tsrsOutputMu.Lock()
	name := t.Name()
	tsrsNames[name]++
	if count := tsrsNames[name]; count > 1 {
		name += "#" + strconv.Itoa(count)
	}
	tsrsOutputMu.Unlock()
	return &TsrsRecording{header: map[string]any{
		"name":            name,
		"files":           encoded,
		"caseSensitive":   caseSensitive,
		"inferredOptions": stdjson.RawMessage(inferred),
	}}
}

func (r *TsrsRecording) add(event any) {
	r.mu.Lock()
	r.events = append(r.events, event)
	r.mu.Unlock()
}

// TsrsWrite records the harness writing a file.
func (r *TsrsRecording) TsrsWrite(path string, content string) {
	if r != nil {
		r.add(map[string]any{"write": path, "text": tsrsText(content)})
	}
}

// TsrsRemove records the harness removing a file or directory.
func (r *TsrsRecording) TsrsRemove(path string) {
	if r != nil {
		r.add(map[string]any{"remove": path})
	}
}

// tsrsFrames records each framed message written through it ("c" from the
// client, "s" from the server) before passing the bytes on.
type tsrsFrames struct {
	w      io.Writer
	r      *TsrsRecording
	key    string
	buffer []byte
}

func (f *tsrsFrames) Write(p []byte) (int, error) {
	f.buffer = append(f.buffer, p...)
	for {
		header := bytes.Index(f.buffer, []byte("\r\n\r\n"))
		if header < 0 {
			break
		}
		length := -1
		for _, line := range strings.Split(string(f.buffer[:header]), "\r\n") {
			if value, ok := strings.CutPrefix(line, "Content-Length: "); ok {
				length, _ = strconv.Atoi(value)
			}
		}
		if length < 0 {
			panic("tsrs: a frame without a length")
		}
		end := header + 4 + length
		if len(f.buffer) < end {
			break
		}
		body := append([]byte(nil), f.buffer[header+4:end]...)
		f.r.add(map[string]any{f.key: stdjson.RawMessage(body)})
		f.buffer = f.buffer[end:]
	}
	return f.w.Write(p)
}

func (r *TsrsRecording) wrap(w io.Writer, key string) io.Writer {
	if r == nil {
		return w
	}
	return &tsrsFrames{w: w, r: r, key: key}
}

// TsrsEnd writes the recording as one JSON line.
func (r *TsrsRecording) TsrsEnd() {
	if r == nil {
		return
	}
	r.mu.Lock()
	r.header["events"] = r.events
	var line bytes.Buffer
	encoder := stdjson.NewEncoder(&line)
	encoder.SetEscapeHTML(false)
	err := encoder.Encode(r.header)
	r.mu.Unlock()
	if err != nil {
		panic(err)
	}
	tsrsOutputMu.Lock()
	defer tsrsOutputMu.Unlock()
	output, err := os.OpenFile(os.Getenv(tsrsVariable), os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0o644)
	if err != nil {
		panic(err)
	}
	defer output.Close()
	if _, err := output.Write(line.Bytes()); err != nil {
		panic(err)
	}
}
