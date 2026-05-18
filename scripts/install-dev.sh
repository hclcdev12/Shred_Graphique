#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

PREFIX="${PREFIX:-/usr/local}"
BINARY="$ROOT_DIR/target/release/shred-graphique"
HELPER_BINARY="$ROOT_DIR/target/release/shred-graphique-helper"
POLICY_SRC="$ROOT_DIR/packaging/shred-graphique.policy"
HELPER_INSTALL="$PREFIX/bin/shred-graphique-helper"

say() { printf "%s\n" "$*"; }
fail() { say "ERROR: $*"; exit 1; }

if [[ ! -f "$BINARY" ]]; then
    fail "Binaire introuvable: $BINARY (executez d'abord: cargo build --release)"
fi
if [[ ! -f "$HELPER_BINARY" ]]; then
    fail "Helper introuvable: $HELPER_BINARY"
fi
if [[ ! -f "$POLICY_SRC" ]]; then
    fail "Policy polkit introuvable: $POLICY_SRC"
fi

TMP_POLICY="$(mktemp)"
trap 'rm -f "$TMP_POLICY"' EXIT

sed "s|/usr/bin/shred-graphique-helper|$HELPER_INSTALL|g" "$POLICY_SRC" > "$TMP_POLICY"

say "Installation dev sous $PREFIX (sudo requis)..."
sudo install -d "$PREFIX/bin" "$PREFIX/share/polkit-1/actions"
sudo install -m 0755 "$BINARY" "$PREFIX/bin/shred-graphique"
sudo install -m 0755 "$HELPER_BINARY" "$PREFIX/bin/shred-graphique-helper"
sudo install -m 0644 "$TMP_POLICY" "$PREFIX/share/polkit-1/actions/shred-graphique.policy"

say "Installe:"
say "  $PREFIX/bin/shred-graphique"
say "  $PREFIX/bin/shred-graphique-helper"
say "  $PREFIX/share/polkit-1/actions/shred-graphique.policy"
say ""
say "Lancement: make run"
say "  (SHRED_GRAPHIQUE_HELPER=$HELPER_INSTALL)"
