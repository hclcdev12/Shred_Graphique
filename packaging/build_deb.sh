#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PKG_DIR="$ROOT_DIR/packaging"
STAGE_DIR="$PKG_DIR/stage"
DIST_DIR="$ROOT_DIR/dist"
BINARY="$ROOT_DIR/target/release/shred-graphique"
HELPER_BINARY="$ROOT_DIR/target/release/shred-graphique-helper"

VERSION="$(grep -m1 '^version' "$ROOT_DIR/Cargo.toml" | sed -E 's/version = "([^"]+)"/\1/')"
ARCH="$(dpkg --print-architecture 2>/dev/null || echo "amd64")"

mkdir -p "$DIST_DIR"

echo "Building release binary..."
( cd "$ROOT_DIR" && cargo build --release )

if [[ ! -f "$BINARY" ]]; then
    echo "Release binary not found at: $BINARY"
    exit 1
fi
if [[ ! -f "$HELPER_BINARY" ]]; then
    echo "Helper binary not found at: $HELPER_BINARY"
    exit 1
fi

rm -rf "$STAGE_DIR"
mkdir -p \
    "$STAGE_DIR/DEBIAN" \
    "$STAGE_DIR/usr/bin" \
    "$STAGE_DIR/usr/share/applications" \
    "$STAGE_DIR/usr/share/polkit-1/actions" \
    "$STAGE_DIR/usr/share/icons/hicolor/scalable/apps" \
    "$STAGE_DIR/usr/share/doc/shred-graphique"

install -m 0755 "$BINARY" "$STAGE_DIR/usr/bin/shred-graphique"
install -m 0755 "$HELPER_BINARY" "$STAGE_DIR/usr/bin/shred-graphique-helper"
install -m 0644 "$PKG_DIR/shred-graphique.desktop" \
    "$STAGE_DIR/usr/share/applications/shred-graphique.desktop"
install -m 0644 "$ROOT_DIR/content/device-hdd.svg" \
    "$STAGE_DIR/usr/share/icons/hicolor/scalable/apps/shred-graphique.svg"
install -m 0644 "$PKG_DIR/shred-graphique.policy" \
    "$STAGE_DIR/usr/share/polkit-1/actions/shred-graphique.policy"

if [[ -f "$ROOT_DIR/LICENSE" ]]; then
    install -m 0644 "$ROOT_DIR/LICENSE" "$STAGE_DIR/usr/share/doc/shred-graphique/LICENSE"
fi
if [[ -f "$ROOT_DIR/docs/USER_GUIDE.md" ]]; then
    install -m 0644 "$ROOT_DIR/docs/USER_GUIDE.md" \
        "$STAGE_DIR/usr/share/doc/shred-graphique/USER_GUIDE.md"
fi

cat > "$STAGE_DIR/DEBIAN/control" <<EOF
Package: shred-graphique
Version: $VERSION
Section: utils
Priority: optional
Architecture: $ARCH
Depends: libgtk-4-1, policykit-1, coreutils, smartmontools, util-linux
Maintainer: Shred Graphique <noreply@example.com>
Description: Secure disk wiping GUI (GTK4)
 Graphical interface to securely wipe disks using shred.
EOF

echo "Building .deb package..."
dpkg-deb --build "$STAGE_DIR" "$DIST_DIR/shred-graphique_${VERSION}_${ARCH}.deb"

echo "Package created at: $DIST_DIR/shred-graphique_${VERSION}_${ARCH}.deb"
