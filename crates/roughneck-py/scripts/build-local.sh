#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
CRATE_DIR="$ROOT/crates/roughneck-py"
TARGET_DIR="$ROOT/target/debug"
EXT_SUFFIX="$(python3 -c 'import sysconfig; print(sysconfig.get_config_var("EXT_SUFFIX") or ".so")')"

case "$(uname -s)" in
  Darwin)
    SRC_LIB="$TARGET_DIR/libroughneck_py.dylib"
    ;;
  Linux)
    SRC_LIB="$TARGET_DIR/libroughneck_py.so"
    ;;
  MINGW*|MSYS*|CYGWIN*|Windows_NT)
    SRC_LIB="$TARGET_DIR/roughneck_py.dll"
    ;;
  *)
    echo "unsupported platform: $(uname -s)" >&2
    exit 1
    ;;
esac

cargo build -p roughneck-py --manifest-path "$ROOT/Cargo.toml"
mkdir -p "$CRATE_DIR/python/roughneck_py"
cp "$SRC_LIB" "$CRATE_DIR/python/roughneck_py/_roughneck_py${EXT_SUFFIX}"
