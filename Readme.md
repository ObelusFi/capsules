<img src="https://raw.githubusercontent.com/ObelusFi/logos_and_icons/refs/heads/main/capsules_banner.svg" alt="Capsules">

Capsules wraps your app, its files, and a tiny supervisor into a single binary
per target platform. Ship the executable to any supported machine and start the
whole workload with one command—no installers, no extra tooling.

## Why Capsules?

- **Bundles everything** – configs, env vars, and filesystem assets live inside the binary alongside the supervisor.
- **Ships once per target** – one command produces a single executable for Linux, macOS, or Windows triples.
- **Supervisor included** – the embedded supervisor lets you start, stop, restart, and inspect processes with a single CLI.
- **Lock down payloads** – compile with `-p <password>` to encrypt embedded data; without the password the capsule payload stays sealed.

## Quick Start

1. **Describe your workload** using JSON or TOML (see `schemas/capsule.json`).
   ```jsonc
   {
     "version": "1.0.0", // surfaced via `capsule daemon status`
     "env": {
       // global env vars applied to every process
       "NODE_ENV": "production",
       "API_URL": "https://api.example.com"
     },
     "files": {
       // top-level files extracted before processes start
       "scripts/setup.sh": "bin/setup.sh"
     },
     "processes": {
       "worker": {
         "cmd": "bun", // command to run
         "args": ["run", "index.js"], // optional arguments
         "cwd": "worker", // working directory created automatically
         "files": {
           // process-specific assets bundled into its cwd
           "index.js": "index.js",
           "config.json": "config.json"
         },
         "env": {
           // per-process env vars override/extend globals
           "WORKER_QUEUE": "high"
         },
         "restart_policy": "on_failure", // never | always | on_failure
         "restart_delay": 5000 // ms to wait before restarting
       },
       "scheduler": {
         "cmd": "./bin/setup.sh",
         "args": ["--watch"],
         "cwd": "scheduler",
         "files": {
           "jobs.toml": "jobs.toml"
         },
         "env": {
           "SCHEDULER_INTERVAL": "30s"
         },
         "restart_policy": "always",
         "restart_delay": 2000
       }
     }
   }
   ```
2. **Compile it** with the Capsules compiler:
   ```bash
   capsule -i capsule.json -t x86_64-apple-darwin -p secret -o ./capsule-macos
   ```
   The command parses your config, embeds `scripts/setup.sh`, the worker files
   (`worker/index.js`, `worker/config.json`), scheduler assets (`scheduler/jobs.toml`),
   and any other referenced files, encrypts them when `-p` is set, and writes the
   executable you distribute.
3. **Run the capsule** on the target machine:
   ```bash
   ./capsule-macos daemon start
   # prompts for the password if you compiled with -p
   # later on, inspect the workload
   ./capsule-macos proc list
   # example output
   Name       Status            CPU%   Memory   IO Read   IO Write   Uptime   Restarts
   worker     Running pid 4123  12.4%  64.0 MiB 1.2 MiB   860 KiB    02m15s   0
   scheduler  Running pid 4198  4.7%   22.0 MiB 640 KiB   120 KiB    07m42s   2
   ```
   Everything (files, env vars, restart policies) is already inside the binary.

## Download Latest Release

Grab the latest prebuilt Capsule binary:

```bash
curl -sSL https://raw.githubusercontent.com/ObelusFi/capsules/refs/heads/master/scripts/install_capsule_linux.sh | bash
```

```bash
curl -sSL https://raw.githubusercontent.com/ObelusFi/capsules/refs/heads/master/scripts/install_capsule_mac.sh | bash
```

```bash
iwr https://raw.githubusercontent.com/ObelusFi/capsules/refs/heads/master/scripts/install_capsule_windows.sh | iex
```

## Capsule CLI Cheatsheet

```
./capsule daemon start        # boot supervisor & bundled processes
./capsule daemon status       # capsule + runtime versions
./capsule daemon stop         # tear everything down
./capsule proc list           # CPU, memory, IO, uptime, restarts
./capsule proc kill <name>    # terminate a named process
./capsule proc restart <name> # restart a process
./capsule proc kill-all       # kills all processes
./capsule version             # print runtime version
```

All commands talk to the supervisor over localhost UDP using the port stored in
`.capsule/capsule.port`. `daemon clean` exists but is not implemented yet.

## Compiling from Source

Working on Capsules itself or cutting custom builds?

1. **Toolchain** – install the Rust toolchain (Edition 2024) and add the targets you plan to support, e.g. `rustup target add x86_64-unknown-linux-musl`.
2. **Build runtimes** – compile each runtime you want to embed:
   ```bash
   cargo build --release -p capsules_runtime --target x86_64-unknown-linux-musl
   ```
3. **Build the compiler** – once the runtimes exist under `target/<triple>`, compile the CLI so it can bundle them:
   ```bash
   cargo build --release -p capsules_compiler
   ```
4. **Optional helper** – `./scripts/build_all.sh` loops over every supported target, builds runtimes + compilers, and copies the resulting binaries into `./builds/`.

## Repository Layout

- `capsules_lib/` – shared types (`Capsule`, `Process`), encryption helpers,
  table formatting.
- `capsules_runtime/` – supervisor CLI bundled inside every capsule.
- `capsules_compiler/` – the CLI you run to create capsule binaries.
- `scripts/` – helper automation (cross-compilation, release helpers, git hooks).
- `schemas/` – JSON Schema describing the capsule format.
- `test_project/` – sample capsule definition and assets.

## Development Notes

- `cargo test -p capsules_lib` exercises serialization helpers and schema-aware
  code.
- `scripts/build_all.sh` cross-builds every supported triple and copies
  binaries into `builds/`.
- `scripts/install_hooks.sh`, `scripts/next_version.sh`, and
  `scripts/semantic_release.sh` support the release workflow.
- `CHANGELOG.md` tracks released versions and notable changes.

## License

Capsules is licensed under the MIT License (`Licence`).
