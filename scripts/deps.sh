#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

say() { printf "%s\n" "$*"; }
fail() { say "ERROR: $*"; exit 1; }

APT_PACKAGES=(
    build-essential
    pkg-config
    curl
    libgtk-4-dev
    libglib2.0-dev
    policykit-1
    coreutils
    util-linux
    smartmontools
)

REQUIRED_COMMANDS=(
    shred
    pkexec
    lsblk
    findmnt
    timeout
    smartctl
    blkid
    pkg-config
    curl
)

load_cargo_env() {
    if [[ -f "$HOME/.cargo/env" ]]; then
        # shellcheck disable=SC1091
        source "$HOME/.cargo/env"
    fi
    export PATH="$HOME/.cargo/bin:$PATH"
}

have_cmd() {
    command -v "$1" >/dev/null 2>&1
}

is_pkg_installed() {
    dpkg -s "$1" >/dev/null 2>&1
}

ensure_apt() {
    if ! command -v apt-get >/dev/null 2>&1; then
        fail "apt-get introuvable. Ce script supporte uniquement Ubuntu/Debian."
    fi
}

install_missing_packages() {
    local missing=()
    local pkg

    for pkg in "${APT_PACKAGES[@]}"; do
        if ! is_pkg_installed "$pkg"; then
            missing+=("$pkg")
        fi
    done

    if [[ ${#missing[@]} -eq 0 ]]; then
        say "Paquets apt deja installes."
        return 0
    fi

    say "Installation des paquets manquants: ${missing[*]}"
    sudo apt-get update
    sudo apt-get install -y "${missing[@]}"
}

ensure_rust_toolchain() {
    load_cargo_env

    if have_cmd cargo && have_cmd rustc; then
        return 0
    fi

    say "Installation du toolchain Rust (rustup)..."
    if ! have_cmd rustup; then
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable
    fi

    load_cargo_env

    if have_cmd rustup; then
        rustup toolchain install stable
        rustup default stable
    fi

    load_cargo_env

    if ! have_cmd cargo || ! have_cmd rustc; then
        fail "cargo/rustc toujours absents. Executez: source \"\$HOME/.cargo/env\""
    fi

    say "Rust toolchain pret."
}

verify_environment() {
    local ok=true
    local cmd

    load_cargo_env

    for cmd in "${REQUIRED_COMMANDS[@]}"; do
        if ! have_cmd "$cmd"; then
            say "  manquant: $cmd"
            ok=false
        fi
    done

    if ! have_cmd cargo; then
        say "  manquant: cargo"
        ok=false
    fi
    if ! have_cmd rustc; then
        say "  manquant: rustc"
        ok=false
    fi

    if ! pkg-config --exists gtk4 2>/dev/null; then
        say "  manquant: bibliotheque gtk4 (pkg-config gtk4)"
        ok=false
    fi

    if [[ "$ok" == "true" ]]; then
        return 0
    fi
    return 1
}

print_success() {
    say ""
    say "Environnement OK."
    say "  gtk4: $(pkg-config --modversion gtk4)"
    say "  cargo: $(command -v cargo)"
    say ""
    say "Prochaines etapes:"
    say "  make dev    # build release + install locale"
    say "  make run    # lancer l'application"
}

main() {
    ensure_apt
    load_cargo_env

    install_missing_packages
    ensure_rust_toolchain

    say "Verification finale..."
    if verify_environment; then
        print_success
        exit 0
    fi

    say ""
    fail "Dependances incompletes apres installation. Verifiez sudo/apt puis relancez: make deps"
}

main "$@"
