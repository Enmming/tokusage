#!/usr/bin/env bash
#
# tokusage installer. Downloads the latest release binary from GitHub and
# puts it in ~/.local/bin. Usage:
#
#   curl -sSL https://github.com/gd/tokusage/releases/latest/download/install.sh | bash
#
# Environment overrides:
#   TOKUSAGE_VERSION   : pin to a specific tag like "v0.1.0" (default: latest)
#   TOKUSAGE_REPO      : override "gd/tokusage"
#   TOKUSAGE_BIN_DIR   : override "$HOME/.local/bin"

set -euo pipefail

REPO="${TOKUSAGE_REPO:-gd/tokusage}"
BIN_DIR="${TOKUSAGE_BIN_DIR:-$HOME/.local/bin}"
VERSION="${TOKUSAGE_VERSION:-latest}"

log() { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
err() { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }

command -v curl >/dev/null || err "curl is required"
command -v tar  >/dev/null || err "tar is required"

# Detect platform.
UNAME_S="$(uname -s)"
UNAME_M="$(uname -m)"
case "$UNAME_S-$UNAME_M" in
  Darwin-arm64)     TARGET="aarch64-apple-darwin" ;;
  Darwin-x86_64)    TARGET="x86_64-apple-darwin" ;;
  Linux-x86_64)     TARGET="x86_64-unknown-linux-musl" ;;
  Linux-aarch64)    TARGET="aarch64-unknown-linux-musl" ;;
  Linux-arm64)      TARGET="aarch64-unknown-linux-musl" ;;
  *) err "unsupported platform: $UNAME_S $UNAME_M" ;;
esac

# Resolve version to a concrete tag so we can build the filename.
if [ "$VERSION" = "latest" ]; then
  log "resolving latest release tag..."
  VERSION="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
    | grep -m1 '"tag_name"' \
    | sed -E 's/.*"tag_name":[[:space:]]*"([^"]+)".*/\1/')"
  [ -n "$VERSION" ] || err "could not determine latest version from GitHub API"
fi

STAGE="tokusage-${VERSION}-${TARGET}"
TARBALL="${STAGE}.tar.gz"
URL="https://github.com/${REPO}/releases/download/${VERSION}/${TARBALL}"
SHA_URL="${URL}.sha256"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

log "downloading ${VERSION} for ${TARGET}"
curl -fsSL "$URL" -o "$TMP/$TARBALL"

# Checksum: best effort. If the sha256 sidecar 404s (rare), warn and continue.
if curl -fsSL "$SHA_URL" -o "$TMP/${TARBALL}.sha256" 2>/dev/null; then
  log "verifying sha256..."
  EXPECTED="$(awk '{print $1}' "$TMP/${TARBALL}.sha256")"
  if command -v shasum >/dev/null; then
    ACTUAL="$(shasum -a 256 "$TMP/$TARBALL" | awk '{print $1}')"
  else
    ACTUAL="$(sha256sum "$TMP/$TARBALL" | awk '{print $1}')"
  fi
  [ "$EXPECTED" = "$ACTUAL" ] || err "sha256 mismatch: expected $EXPECTED got $ACTUAL"
else
  printf '\033[1;33mwarn:\033[0m no sha256 sidecar available; skipping checksum\n'
fi

log "unpacking..."
tar xzf "$TMP/$TARBALL" -C "$TMP"

mkdir -p "$BIN_DIR"
install -m 0755 "$TMP/$STAGE/tokusage" "$BIN_DIR/tokusage"

# Strip macOS Gatekeeper quarantine attribute on files downloaded by curl.
# Best effort; silent if already unquarantined or not on macOS.
if [ "$UNAME_S" = "Darwin" ]; then
  xattr -d com.apple.quarantine "$BIN_DIR/tokusage" 2>/dev/null || true
fi

log "installed $BIN_DIR/tokusage"
"$BIN_DIR/tokusage" --version || true

case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *)
    printf '\n'
    printf '\033[1;33mNote:\033[0m %s is not in your PATH.\n' "$BIN_DIR"
    printf '     Add this line to ~/.zshrc or ~/.bashrc:\n'
    printf '         export PATH="%s:$PATH"\n' "$BIN_DIR"
    printf '     Then reopen your terminal or run: source ~/.zshrc\n\n'
    ;;
esac

cat <<'EOF'
Next steps:
  tokusage login   # configure your company API endpoint + token
  tokusage init    # install launchd scheduler (+ optional Claude Code hook)
  tokusage submit  # send the first payload immediately
EOF
