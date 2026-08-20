#!/usr/bin/env bash
#
# install-obscura.sh — fetch and install the Obscura headless browser binary
# needed by the e2e (Playwright) tests.
#
# Obscura is a lightweight Rust headless browser that is a drop-in replacement
# for headless Chrome. It is distributed as a GitHub release tarball/zip (there
# is no published crate for the browser binary itself). This script detects the
# host OS/arch, downloads the matching release, and installs `obscura` (and its
# `obscura-worker`) into a directory of your choice.
#
# Usage:
#   scripts/install-obscura.sh                 # install to ./bin (repo-local)
#   OBSCURA_BIN_DIR=~/.local/bin scripts/install-obscura.sh
#   OBSCURA_VERSION=v0.1.2 scripts/install-obscura.sh
#   OBSCURA_VARIANT=-no-render-stealth scripts/install-obscura.sh
#
# Env vars:
#   OBSCURA_VERSION   Release tag to install (default: latest).
#   OBSCURA_VARIANT   Asset suffix: "" (full), -no-render, -stealth,
#                     -no-render-stealth (default: -no-render, which runs
#                     headless without a display — ideal for dev and CI).
#   OBSCURA_BIN_DIR   Where to place the binary (default: <repo>/bin).
#
# After installing, the e2e harness finds the binary automatically from
# ./bin (or PATH, or $OBSCURA_BIN). If it is installed somewhere else, set
# OBSCURA_BIN=<path> when running the tests.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="${OBSCURA_VERSION:-latest}"
VARIANT="${OBSCURA_VARIANT:--no-render}"
BIN_DIR="${OBSCURA_BIN_DIR:-$REPO_ROOT/bin}"

# Map uname -> Obscura asset platform tag.
detect_os() {
    case "$(uname -s)" in
        Linux)  echo "linux" ;;
        Darwin) echo "macos" ;;
        MINGW*|MSYS*|CYGWIN*) echo "windows" ;;
        *) echo "unsupported" ;;
    esac
}
detect_arch() {
    case "$(uname -m)" in
        x86_64|amd64) echo "x86_64" ;;
        aarch64|arm64) echo "aarch64" ;;
        *) echo "unsupported" ;;
    esac
}

OS="$(detect_os)"
ARCH="$(detect_arch)"
if [ "$OS" = "unsupported" ] || [ "$ARCH" = "unsupported" ]; then
    echo "error: unsupported platform ($(uname -s)/$(uname -m))." >&2
    echo "Install Obscura manually from https://github.com/h4ckf0r0day/obscura/releases" >&2
    exit 1
fi

# Resolve the release tag to download. Note: do not pipe curl straight into
# `grep -m1`/`head -1` — those exit after one match and close the pipe, making
# curl fail with SIGPIPE under `set -o pipefail`. sed consumes all input first.
if [ "$VERSION" = "latest" ]; then
    VERSION="$(curl -fsSL https://api.github.com/repos/h4ckf0r0day/obscura/releases/latest \
        | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -1)"
fi

EXT="tar.gz"; [ "$OS" = "windows" ] && EXT="zip"
ASSET="obscura-${ARCH}-${OS}${VARIANT}.${EXT}"
URL="https://github.com/h4ckf0r0day/obscura/releases/download/${VERSION}/${ASSET}"

echo ">> Obscura ${VERSION} for ${ARCH}-${OS} (variant '${VARIANT}')"
echo ">> Downloading ${URL}"

mkdir -p "$BIN_DIR"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

curl -fsSL -o "$TMP_DIR/$ASSET" "$URL"

if [ "$EXT" = "zip" ]; then
    (cd "$TMP_DIR" && unzip -q "$ASSET")
else
    tar -xzf "$TMP_DIR/$ASSET" -C "$TMP_DIR"
fi

# The archive contains `obscura` and `obscura-worker`.
install -m 0755 "$TMP_DIR/obscura" "$BIN_DIR/obscura"
if [ -f "$TMP_DIR/obscura-worker" ]; then
    install -m 0755 "$TMP_DIR/obscura-worker" "$BIN_DIR/obscura-worker"
fi

echo ">> Installed obscura to $BIN_DIR"
"$BIN_DIR/obscura" --version

echo
echo "The e2e harness discovers the binary in this order:"
echo "  1. \$OBSCURA_BIN (explicit path)"
echo "  2. <repo>/bin (this script's default)"
echo "  3. PATH"
echo
echo "If it is elsewhere, run tests with:  OBSCURA_BIN=/path/to/obscura cargo test -p e2e"
