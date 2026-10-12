import subprocess, time, statistics, sys, os
T = "/Users/hiramatsu/dev/tsc-rs/target/typescript7/bin/tsc-aa814927"
R = "/Users/hiramatsu/dev/tsc-rs-p34/target/release/tsc-rs"
base = os.path.expanduser("~/dev/real-projects.noindex/content-mapper")
projects = [("site", "."), ("starlight", "tsconfig.cm.json"), ("vitesse", "tsconfig.cm.json"), ("soybean", "tsconfig.cm.json")]
runs = int(sys.argv[1]) if len(sys.argv) > 1 else 7
for name, cfg in projects:
    row = [name]
    for tool in (T, R):
        times = []
        for i in range(runs + 1):
            start = time.perf_counter()
            subprocess.run([tool, "-p", cfg, "--runExternalCode", "--noEmit", "--pretty", "false"], cwd=os.path.join(base, name), stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            elapsed = (time.perf_counter() - start) * 1000
            if i > 0:
                times.append(elapsed)
        row.append(f"{statistics.median(times):.0f}")
    print(f"{row[0]:10} tsgo {row[1]:>5} ms   tsc-rs {row[2]:>5} ms   ratio {float(row[2])/float(row[1]):.2f}")
