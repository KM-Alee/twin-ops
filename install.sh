#!/bin/sh
# Installs the twin binary for the current user. It never uses sudo and touches nothing
# outside the install directory.
#
#   curl -fsSL https://github.com/KM-Alee/twin-ops/releases/latest/download/install.sh | sh
#
# Environment: TWIN_VERSION (default: latest), TWIN_INSTALL_DIR (default: ~/.local/bin),
#              TWIN_DOWNLOAD_BASE (mirror of the release download URL; requires TWIN_VERSION)
set -eu

repo="KM-Alee/twin-ops"
install_dir="${TWIN_INSTALL_DIR:-$HOME/.local/bin}"

fail() {
    printf 'twin install: %s\n' "$1" >&2
    exit 1
}

[ "$(uname -s)" = "Linux" ] || fail "twin supports Linux only"
case "$(uname -m)" in
    x86_64 | amd64) arch="x86_64" ;;
    aarch64 | arm64) arch="aarch64" ;;
    *) fail "unsupported architecture $(uname -m)" ;;
esac

if command -v curl >/dev/null 2>&1; then
    fetch() { curl -fsSL "$1" -o "$2"; }
    fetch_text() { curl -fsSL "$1"; }
elif command -v wget >/dev/null 2>&1; then
    fetch() { wget -q "$1" -O "$2"; }
    fetch_text() { wget -qO- "$1"; }
else
    fail "curl or wget is required"
fi

version="${TWIN_VERSION:-}"
if [ -z "$version" ]; then
    version=$(fetch_text "https://api.github.com/repos/$repo/releases/latest" |
        sed -n 's/.*"tag_name": *"v\{0,1\}\([^"]*\)".*/\1/p' | head -n 1)
    [ -n "$version" ] || fail "could not determine the latest release"
fi
version="${version#v}"

name="twin-${version}-${arch}-linux"
base="${TWIN_DOWNLOAD_BASE:-https://github.com/$repo/releases/download/v${version}}"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

printf 'Downloading twin %s for %s...\n' "$version" "$arch"
fetch "$base/${name}.tar.gz" "$work/${name}.tar.gz" || fail "download failed"
fetch "$base/${name}.tar.gz.sha256" "$work/${name}.tar.gz.sha256" || fail "checksum download failed"

expected=$(cut -d' ' -f1 "$work/${name}.tar.gz.sha256")
if command -v sha256sum >/dev/null 2>&1; then
    actual=$(sha256sum "$work/${name}.tar.gz" | cut -d' ' -f1)
else
    actual=$(shasum -a 256 "$work/${name}.tar.gz" | cut -d' ' -f1)
fi
[ "$expected" = "$actual" ] || fail "checksum mismatch; refusing to install"

tar -xzf "$work/${name}.tar.gz" -C "$work"
mkdir -p "$install_dir"
cp "$work/${name}/twin" "$install_dir/twin"
chmod 0755 "$install_dir/twin"

printf 'Installed %s\n' "$install_dir/twin"
case ":$PATH:" in
    *":$install_dir:"*) ;;
    *) printf 'Add %s to your PATH to run twin directly.\n' "$install_dir" ;;
esac
printf 'Next: twin setup claude-code   (or codex, cursor, gemini, vscode, generic)\n'
