#!/usr/bin/env bash
# Removes the files installed by install.sh. Projects in ~/.local/share/velomov are kept.
set -euo pipefail
PREFIX="${PREFIX:-$HOME/.local}"
rm -f "$PREFIX/bin/velomov" \
      "$PREFIX/share/icons/hicolor/scalable/apps/velomov.svg" \
      "$PREFIX/share/applications/velomov.desktop"
rm -rf "$PREFIX/lib/velomov"
for s in 32 48 64 128 256 512; do
    rm -f "$PREFIX/share/icons/hicolor/${s}x${s}/apps/velomov.png"
done
command -v update-desktop-database >/dev/null && update-desktop-database "$PREFIX/share/applications" 2>/dev/null || true
echo "VeloMov removed. Your projects and downloaded speech models are still in ~/.local/share/velomov"
echo "(delete that folder to remove them)."
echo "Caches: ~/.cache/velomov   Settings: ~/.config/velomov"
