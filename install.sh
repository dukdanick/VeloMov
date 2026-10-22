#!/usr/bin/env bash
# VeloMov installer: builds a release binary and installs it for the current user (no root needed).
set -euo pipefail
cd "$(dirname "$0")"

PREFIX="${PREFIX:-$HOME/.local}"
need() { command -v "$1" >/dev/null 2>&1; }

missing=()
need cargo      || missing+=("rust (cargo)")
need cc         || missing+=("C compiler (gcc)")
need pkg-config || missing+=("pkg-config")
need ffmpeg     || missing+=("ffmpeg")
need ffprobe    || missing+=("ffprobe (ffmpeg)")
need c++        || missing+=("C++ compiler (g++)")
need curl       || missing+=("curl")
if need pkg-config && ! pkg-config --exists alsa; then
    missing+=("ALSA development headers")
fi
if ((${#missing[@]})); then
    echo "Missing: ${missing[*]}"
    echo "  Arch/CachyOS:   sudo pacman -S --needed rust ffmpeg alsa-lib pkgconf base-devel curl shaderc"
    echo "  Debian/Ubuntu:  sudo apt install cargo build-essential ffmpeg libasound2-dev pkg-config curl glslc"
    echo "  Fedora:         sudo dnf install cargo gcc pkgconf-pkg-config alsa-lib-devel ffmpeg-free"
    echo "                  (for H.264/H.265: enable RPM Fusion, then: sudo dnf swap ffmpeg-free ffmpeg --allowerasing)"
    exit 1
fi

echo "==> Building VeloMov (release)…"
cargo build --release --locked
BIN="${CARGO_TARGET_DIR:-target}/release/velomov"

echo "==> Installing to $PREFIX"
install -Dm755 "$BIN" "$PREFIX/bin/velomov"
for s in 32 48 64 128 256 512; do
    install -Dm644 "assets/icons/velomov-$s.png" "$PREFIX/share/icons/hicolor/${s}x${s}/apps/velomov.png"
done
install -Dm644 assets/velomov.svg "$PREFIX/share/icons/hicolor/scalable/apps/velomov.svg"
install -d "$PREFIX/share/applications"
sed "s|^Exec=velomov|Exec=$PREFIX/bin/velomov|" packaging/velomov.desktop \
    > "$PREFIX/share/applications/velomov.desktop"
chmod 644 "$PREFIX/share/applications/velomov.desktop"
command -v update-desktop-database >/dev/null && update-desktop-database "$PREFIX/share/applications" 2>/dev/null || true
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q -t "$PREFIX/share/icons/hicolor" 2>/dev/null || true

# speech recognition for auto captions (whisper.cpp; with the GPU through Vulkan when possible)
if [ "${SKIP_WHISPER:-0}" != 1 ] && [ ! -x "$PREFIX/lib/velomov/whisper/whisper-cli" ]; then
    echo "==> Building the speech recognition engine for auto captions (whisper.cpp)…"
    if ! PREFIX="$PREFIX" packaging/linux/build-whisper.sh; then
        echo "Note: whisper.cpp could not be built; auto captions need whisper-cli in PATH."
        echo "      Try again later with: PREFIX=$PREFIX packaging/linux/build-whisper.sh"
    fi
fi

case ":$PATH:" in
    *":$PREFIX/bin:"*) ;;
    *) echo "Note: add $PREFIX/bin to your PATH to run 'velomov' from a terminal." ;;
esac
echo "==> Done. Open VeloMov from the application menu or run: velomov"
