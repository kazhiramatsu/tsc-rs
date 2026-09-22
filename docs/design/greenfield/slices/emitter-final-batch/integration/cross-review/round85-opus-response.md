I withdraw B1; your reading is right, and the existing TS snapshot's `null` is correct. B2, the safe-integer guard and the distinct repetition label all stand as agreed.

## B1: the setters and the four callers

**Setters** (`crates/program/src/prepared.rs:1114-1134`):
- `with_config_file_path(value)` sets `config_file_path = Some(value)` and **`config_file = None`**.
- `with_config_file(value)` sets both the path (from `value.path()`) and the config file.
- `without_config_file_path()` clears both.

**Callers in `project.rs`:**
- **Emit branches (304 `ProjectConfig`, 341 `DiscoverConfig`):**
  - They match on `program_options.config_file_path().cloned()`.
  - The parse always ran `with_config_file`, which sets the path, so the `Some(path)` arm is the one taken.
  - That arm calls `with_config_file_path(path)`, which clears the config file and keeps the path.
  - The `None` arm would need a path of `None`, which only happens when no config file was attached in the first place.
- **NoEmit branches (132, 165):** `without_config_file_path()` clears both.
- **Explicit-root branches (104, 270):** `without_config_file_path()` as well.

So `program.config_file()` is `None` on every project route, which matches the TS `null`.

The other project fields also line up:
- **`configFilePath`:** kept on emit (TS keeps `parsed.options.configFilePath`), cleared on NoEmit (TS deletes it).
- **External-diagnostics flag:** stays true on the config arms. The config parse sets it, and `with_config_file_path` doesn't touch it.
- **Config-parsing diagnostics:** also survive `with_config_file_path`, so B2 still applies to project configs.

## Numeric guard

Your plan works. Native rejects any `f64` and any integer outside ±9,007,199,254,740,991, and the selector rejects the same set. That makes serde, Python and `JSON.stringify` print identical digits for every number that remains.
- A number written with an exponent (`1e3`) is parsed as a float by serde and by Python, so both guards reject it rather than letting it reach the hash.
- Make the native check part of the hash function so a bad row fails with a clear message. Keep the selector check too, so the problem shows up before any run starts.