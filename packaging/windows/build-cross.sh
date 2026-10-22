#!/usr/bin/env bash
# Build VeloMov for Windows (x64) on Linux and package it:
#   dist/VeloMov-<version>-windows-x64-portable.zip   (unzip and run, keeps its data beside it)
#   dist/VeloMov-<version>-windows-x64-setup.exe      (installer, needs wine for Inno Setup)
# Everything it needs is downloaded once into ~/.cache/velomov-cross (no root needed):
# rustup + the x86_64-pc-windows-gnullvm target, llvm-mingw, FFmpeg and whisper.cpp for
# Windows, and Inno Setup (run under wine).
#   SKIP_INSTALLER=1  only make the portable zip
set -euo pipefail
cd "$(dirname "$0")/../.."
ROOT=$PWD
X="${VELOMOV_CROSS:-$HOME/.cache/velomov-cross}"
mkdir -p "$X/dl" "$X/ext"
VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
TARGET=x86_64-pc-windows-gnullvm
LLVM_MINGW=llvm-mingw-20261006-ucrt-ubuntu-22.04-x86_64
FFMPEG_ZIP=ffmpeg-n9.0-latest-win64-gpl-shared-9.0

fetch() { [ -s "$X/dl/$2" ] || { echo "==> downloading $2"; curl -fL --retry 3 -o "$X/dl/$2.part" "$1" && mv "$X/dl/$2.part" "$X/dl/$2"; }; }

# ---- toolchain
export CARGO_HOME="$X/cargo" RUSTUP_HOME="$X/rustup"
if [ ! -x "$CARGO_HOME/bin/cargo" ]; then
  fetch https://static.rust-lang.org/rustup/dist/x86_64-unknown-linux-gnu/rustup-init rustup-init
  chmod +x "$X/dl/rustup-init"
  "$X/dl/rustup-init" -y --no-modify-path --profile minimal --default-toolchain stable
fi
"$CARGO_HOME/bin/rustup" target add "$TARGET" >/dev/null 2>&1 || true
if [ ! -d "$X/ext/$LLVM_MINGW" ]; then
  fetch "https://github.com/mstorsjo/llvm-mingw/releases/download/20261006/$LLVM_MINGW.tar.xz" llvm-mingw.tar.xz
  tar xf "$X/dl/llvm-mingw.tar.xz" -C "$X/ext"
fi
export PATH="$CARGO_HOME/bin:$X/ext/$LLVM_MINGW/bin:$PATH"
export CARGO_TARGET_X86_64_PC_WINDOWS_GNULLVM_LINKER=x86_64-w64-mingw32-clang
export CC_x86_64_pc_windows_gnullvm=x86_64-w64-mingw32-clang CXX_x86_64_pc_windows_gnullvm=x86_64-w64-mingw32-clang++
export AR_x86_64_pc_windows_gnullvm=llvm-ar WINDRES=x86_64-w64-mingw32-windres

echo "==> cargo build --release --target $TARGET"
cargo build --release --target "$TARGET"
EXE="target/$TARGET/release/velomov.exe"

# ---- bundled tools
fetch "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/$FFMPEG_ZIP.zip" ffmpeg-win.zip
# whisper.cpp built here with llvm-mingw (no Visual C++ runtime needed, Vulkan GPU backend)
[ -f "$X/whisper-win/whisper-cli.exe" ] || packaging/windows/build-whisper-windows.sh
STAGE="$ROOT/dist/VeloMov-$VERSION-windows-x64"
rm -rf "$STAGE" && mkdir -p "$STAGE/ffmpeg" "$STAGE/whisper"
cp "$EXE" "$STAGE/velomov.exe"
# the C++ runtime and unwinder of llvm-mingw (Windows 10+ has the UCRT itself)
RT="$X/ext/$LLVM_MINGW/x86_64-w64-mingw32/bin"
cp "$RT/libunwind.dll" "$STAGE/"
cp "$X/whisper-win/"* "$RT/libc++.dll" "$RT/libunwind.dll" "$STAGE/whisper/"
TMP=$(mktemp -d)
(cd "$TMP" && 7z x -y "$X/dl/ffmpeg-win.zip" >/dev/null)
FF=$(find "$TMP" -maxdepth 1 -type d -name 'ffmpeg-*' | head -1)
cp "$FF/bin/ffmpeg.exe" "$FF/bin/ffprobe.exe" "$FF"/bin/*.dll "$STAGE/ffmpeg/"
cp "$FF/LICENSE.txt" "$STAGE/ffmpeg/LICENSE.txt" 2>/dev/null || true
rm -rf "$TMP"
cp LICENSE "$STAGE/LICENSE.txt"
cp packaging/windows/LEIA-ME-WINDOWS.txt "$STAGE/LEIA-ME.txt"

# ---- portable zip: portable.txt keeps data in .\data next to velomov.exe
mkdir -p "$ROOT/dist"
PZ="$ROOT/dist/VeloMov-$VERSION-windows-x64-portable.zip"
rm -f "$PZ"
P="$TMP-portable/VeloMov-$VERSION-portable"
mkdir -p "$P" && cp -r "$STAGE"/. "$P/"
printf 'VeloMov portable: projects, settings and caches are kept in the "data" folder next to velomov.exe.\r\nDelete this file to use the normal user folders instead.\r\n' > "$P/portable.txt"
(cd "$(dirname "$P")" && 7z a -tzip -mx=7 "$PZ" "$(basename "$P")" >/dev/null)
rm -rf "$TMP-portable"
echo "==> $PZ"

# ---- installer (Inno Setup under wine)
if [ "${SKIP_INSTALLER:-0}" = 1 ]; then exit 0; fi
export WINEPREFIX="$X/wine" WINEDEBUG=-all
ISCC="$WINEPREFIX/drive_c/InnoSetup/ISCC.exe"
if [ ! -f "$ISCC" ]; then
  fetch "https://github.com/jrsoftware/issrc/releases/download/is-6_7_3/innosetup-6.7.3.exe" innosetup-6.7.3.exe
  wine "$X/dl/innosetup-6.7.3.exe" /VERYSILENT /SUPPRESSMSGBOXES /NORESTART /CURRENTUSER /DIR='C:\InnoSetup' >/dev/null 2>&1 || true
fi
[ -f "$ISCC" ] || { echo "Inno Setup could not be installed under wine; the portable zip is ready."; exit 0; }
wine "$ISCC" /Q "/DAppVersion=$VERSION" "/DSourceDir=$(winepath -w "$STAGE")" "/DOutputDir=$(winepath -w "$ROOT/dist")" \
  "/DRepoDir=$(winepath -w "$ROOT")" "$(winepath -w "$ROOT/packaging/windows/velomov.iss")"
echo "==> $ROOT/dist/VeloMov-$VERSION-windows-x64-setup.exe"
