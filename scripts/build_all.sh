#!/usr/bin/env bash
set -e

ROOT="$(git rev-parse --show-toplevel)"

rm -rf "$ROOT/builds"
mkdir -p "$ROOT/builds"

TARGETS=$(sed -n '/RUNTIME_TARGETS/,/\];/p' capsules_lib/src/lib.rs \
  | sed -n 's/.*("//; s/".*//p')

for t in $TARGETS; do
  echo "Building runtime for $t"
  cargo build --bin capsules_runtime --release --target "$t" || {
    echo "Failed build for target $t"
    exit 1
  }
done

for t in $TARGETS; do
  echo "Building compiler for $t"
  cargo build --bin capsules_compiler --release --target "$t" || {
    echo "Failed build for target $t"
    exit 1
  }
done


mkdir -p builds

for path in target/*/*/capsules_compiler target/*/*/capsules_compiler.exe; do
    [ -f "$path" ] || continue

    triple=$(echo "$path" | cut -d/ -f2)
    filename=$(basename "$path")
    ext=""
    if [[ "$filename" == *.exe ]]; then
      ext=".exe"
    fi

    mkdir -p "$ROOT/builds/$triple/"
    cp "$path" "$ROOT/builds/$triple/capsule$ext"
done

echo "Done."