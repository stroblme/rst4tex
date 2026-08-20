#!/bin/sh
# curl -fsSL https://raw.githubusercontent.com/stroblme/rst4tex/main/install.sh | sh
set -eu

REPO=${REPO:-stroblme/rst4tex}
REF=${REF:-main}
INSTALL_DIR=${INSTALL_DIR:-$HOME/.local/bin}
URL=${URL:-https://github.com/$REPO/archive/$REF.tar.gz}

for c in curl tar make rustc; do
	command -v "$c" >/dev/null || { echo "missing: $c" >&2; exit 1; }
done

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

echo "Fetching $REPO@$REF ..."
curl -fsSL "$URL" | tar xz -C "$tmp" --strip-components=1

echo "Building ..."
make -C "$tmp" INSTALL_DIR="$INSTALL_DIR"

case ":$PATH:" in
	*":$INSTALL_DIR:"*) ;;
	*) echo "Note: $INSTALL_DIR is not in your PATH." ;;
esac
