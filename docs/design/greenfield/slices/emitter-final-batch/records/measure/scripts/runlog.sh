#!/bin/zsh
# usage: runlog.sh <record-dir> <name> [ENV=VAL ...] -- <command...>
# Runs the command demoted (taskpolicy -b nice -n 15, CARGO_BUILD_JOBS=2) from the emitter-final worktree,
# writes <name>.log and <name>.meta.json (argv, env, head, exit, seconds, sha256 of log).
set -u
dir="$1"; name="$2"; shift 2
envs=()
while [ "$1" != "--" ]; do envs+=("$1"); shift; done; shift
mkdir -p "$dir"
cd /Users/hiramatsu/dev/tsc-rs-emitter-final || exit 97
head=$(git rev-parse HEAD); dirty=$(git status --short | wc -l | tr -d ' ')
start=$(date +%s); started_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)
taskpolicy -b nice -n 15 env CARGO_BUILD_JOBS=2 "${envs[@]}" "$@" > "$dir/$name.log" 2>&1
code=$?
end=$(date +%s)
python3 - "$dir" "$name" "$head" "$dirty" "$code" "$((end-start))" "$started_at" "${envs[*]}" "$*" <<'PY'
import json,sys,hashlib
d,name,head,dirty,code,secs,started,envs,argv=sys.argv[1:]
log=open(f"{d}/{name}.log","rb").read()
meta={"name":name,"head":head,"dirty_paths":int(dirty),"env":envs.split() if envs else [],"argv":argv,
      "prefix":"taskpolicy -b nice -n 15 env CARGO_BUILD_JOBS=2","started_at":started,"seconds":int(secs),"exit":int(code),
      "log_sha256":hashlib.sha256(log).hexdigest(),"log_bytes":len(log)}
json.dump(meta,open(f"{d}/{name}.meta.json","w"),indent=2)
print(json.dumps(meta))
PY
exit $code
