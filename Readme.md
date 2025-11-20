# Capsules

Capsules bundles a process graph, its configuration, and the supporting files into a single, self-contained executable. The binary embeds a small supervisor (`capsules_runtime`) together with your capsule manifest, so you can ship repeatable services that start, watch, and manage multiple processes without any external dependencies.

## Repository layout

- `capsules_lib` – shared types, schema generation, encryption helpers, and constants that describe the capsule format and supported runtime targets.
- `capsules_runtime` – the supervisor binary that unpacks files, launches processes, and exposes a small UDP control plane.
- `capsules_compiler` – CLI that reads a capsule manifest, packages its files, optionally encrypts them, and appends them to a target-specific runtime.
- `shcemas/shcema.json` – JSON schema for authoring manifests with editor tooling.
- `test_project` – minimal example capsule used for manual testing.

## How Capsules work

1. **Describe your application** in JSON or TOML (both formats map to `capsules_lib::Capsule`). You can attach files, declare processes, working directories, restart policies, and environment variables.
2. **Build a capsule binary** with `capsules_compiler`. The compiler zips the referenced files, injects them into the manifest, and concatenates the bytes onto a prebuilt runtime for the target triple you choose. You can pass `--password` to encrypt the payload with AES‑GCM.
3. **Run the resulting executable** on the target host. The runtime extracts the packaged files, starts each process, and keeps a lightweight supervisor in the background. The same binary exposes commands to start/stop the supervisor or interact with specific processes (`proc list`, `proc restart`, etc.).

## Building `capsules_compiler`

The compiler’s build script compiles the runtime for every supported target listed in `RUNTIME_TARGETS`, so make sure the toolchains exist locally:

```
rustup target add \
  x86_64-pc-windows-gnu \
  x86_64-unknown-linux-musl \
  x86_64-unknown-linux-gnu \
  aarch64-unknown-linux-gnu \
  aarch64-unknown-linux-musl \
  aarch64-apple-darwin \
  x86_64-apple-darwin
```

Then build the compiler:

```
cargo build -p capsules_compiler --release
```

## Writing a capsule manifest

You can write manifests in JSON or TOML. Use `shcemas/shcema.json` for editor validation. Here’s an annotated JSON example:

```json
{
  "$schema": "./shcemas/shcema.json",
  "version": "1.0.0",
  "env": {
    "GLOBAL_FLAG": "1"
  },
  "files": {
    "config/prod.env": "config/.env"
  },
  "processes": {
    "web": {
      "cmd": "./node/bin/node",
      "args": ["server.js"],
      "cwd": "web",
      "env": {
        "PORT": "3000"
      },
      "restart_policy": "on_failure",
      "restart_delay": 2000,
      "files": {
        "dist/index.js": "server.js"
      }
    }
  }
}
```

Key fields:

| Field       | Type   | Description                                                                                  |
| ----------- | ------ | -------------------------------------------------------------------------------------------- |
| `version`   | SemVer | Capsule version that is echoed by the runtime.                                               |
| `env`       | map    | Environment variables applied globally (currently reserved for future runtime support).      |
| `files`     | map    | Global files copied relative to the runtime’s working directory (`source` → `target`).       |
| `processes` | map    | Named processes the supervisor should start. The name becomes the default working directory. |

Each `process` supports:

| Field            | Description                                                             |
| ---------------- | ----------------------------------------------------------------------- |
| `cmd`            | Executable or script to run.                                            |
| `args`           | Optional argument vector.                                               |
| `cwd`            | Working directory (defaults to the process name, created if necessary). |
| `env`            | Process-specific environment variables (reserved for future use).       |
| `restart_policy` | `never`, `always`, or `on_failure`.                                     |
| `restart_delay`  | Milliseconds to wait before a restart attempt.                          |
| `files`          | Files to place in the process’ working directory.                       |

## Compiling a capsule

Run the compiler against your manifest:

```
target/release/capsules_compiler \
  --input-file ./capsule.json \
  --target aarch64-apple-darwin \
  --output-path ./capsule-macos \
  --password "s3cret"            # optional
```

Options:

- `--target` must be one of the supported triples listed earlier.
- `--password` enables AES‑GCM encryption. The runtime will prompt for the password the first time the supervisor starts.
- If `--output-path` is omitted, the compiler writes `"<manifest-stem>-<target><ext>"` in the manifest directory.

The command writes a standalone executable that includes the runtime, your manifest, and all referenced files.

## Running a capsule

Use the generated binary directly on the target host:

```
./capsule-macos daemon start    # unpack files and start the supervisor
./capsule-macos proc list       # show process table (status, cpu, memory, IO, restarts)
./capsule-macos proc restart web
./capsule-macos proc kill web
./capsule-macos proc kill-all
./capsule-macos daemon status
./capsule-macos daemon stop
./capsule-macos version
```

Behind the scenes the runtime spawns the supervisor (`capsule supervisor`) in the background, writes `.capsule/capsule.port` next to the executable, and uses a UDP socket for CLI commands. Logs from managed processes stream to the caller’s stdout/stderr so you can run capsules under systemd, launchd, or a container entrypoint.

If the payload was encrypted, `daemon start` prompts for the password in the terminal (or reads from stdin when non-interactive).

## Example project

`test_project/` contains a tiny Bun app. From the repo root:

```
cd test_project
cargo build -p capsules_compiler --release
../target/release/capsules_compiler \
  --input-file capsule.json \
  --target aarch64-apple-darwin \
  --output-path capsule-aarch64-apple-darwin
./capsule-aarch64-apple-darwin daemon start
./capsule-aarch64-apple-darwin proc list
./capsule-aarch64-apple-darwin daemon stop
```

## Schema updates

The JSON schema lives under `shcemas/shcema.json`. Regenerate it after changing the capsule struct by running:

```
cargo test -p capsules_lib gen_schema
```

This executes the `gen_schema` test, which rewrites the schema file using `schemars`.

## Contributing

- `cargo fmt` and `cargo clippy` keep style consistent.
- `cargo test --workspace` runs the shared schema test.
- When adding runtime targets update `capsules_lib::RUNTIME_TARGETS` and make sure the necessary toolchains are installed so the compiler build script can locate the artifacts.
