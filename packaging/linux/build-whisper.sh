#!/usr/bin/env bash
# Build whisper.cpp's whisper-cli for VeloMov's auto captions and install it to
# $PREFIX/lib/velomov/whisper (default PREFIX=~/.local). Uses the GPU through Vulkan when
# glslc and the Vulkan loader are present (much faster), else the processor only.
# Needs: a C/C++ compiler, curl, tar. CMake and the Vulkan headers are downloaded if missing.
set -euo pipefail
WHISPER_VERSION="${WHISPER_VERSION:-1.9.5}"
PREFIX="${PREFIX:-$HOME/.local}"
WORK="${WORK:-${XDG_CACHE_HOME:-$HOME/.cache}/velomov-build}"
DEST="$PREFIX/lib/velomov/whisper"
mkdir -p "$WORK" && cd "$WORK"

fetch() { [ -s "$2" ] || curl -fL --retry 3 -o "$2.part" "$1" && { [ -s "$2" ] || mv "$2.part" "$2"; }; }

CMAKE=cmake
if ! command -v cmake >/dev/null; then
  CMV=4.4.4
  fetch "https://github.com/Kitware/CMake/releases/download/v$CMV/cmake-$CMV-linux-x86_64.tar.gz" cmake.tar.gz
  [ -d cmake-$CMV-linux-x86_64 ] || tar xzf cmake.tar.gz
  CMAKE="$WORK/cmake-$CMV-linux-x86_64/bin/cmake"
fi

fetch "https://github.com/ggml-org/whisper.cpp/archive/refs/tags/v$WHISPER_VERSION.tar.gz" whisper-src.tar.gz
rm -rf "whisper.cpp-$WHISPER_VERSION" && tar xzf whisper-src.tar.gz

OPTS=(-DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=OFF -DWHISPER_BUILD_TESTS=OFF -DWHISPER_BUILD_SERVER=OFF
      -DWHISPER_SDL2=OFF -DWHISPER_CURL=OFF -DGGML_NATIVE=ON -DGGML_OPENMP=ON)
GPU="processor only"
if [ "${WHISPER_GPU:-auto}" != "off" ] && command -v glslc >/dev/null && [ -n "$(ldconfig -p 2>/dev/null | grep libvulkan.so || true)" ]; then
  if [ ! -f /usr/include/vulkan/vulkan.h ]; then
    fetch "https://github.com/KhronosGroup/Vulkan-Headers/archive/refs/tags/vulkan-sdk-1.4.363.0.tar.gz" vkheaders.tar.gz
    rm -rf vkh && mkdir vkh && tar xzf vkheaders.tar.gz -C vkh --strip-components=1
    OPTS+=(-DVulkan_INCLUDE_DIR="$WORK/vkh/include")
    VKLIB=$(ldconfig -p | grep -o "/[^ ]*libvulkan.so.1$" | head -1 || true)
    [ -n "$VKLIB" ] && OPTS+=(-DVulkan_LIBRARY="$VKLIB")
  fi
  if ! ls /usr/share/cmake/SPIRV-Headers /usr/lib/cmake/SPIRV-Headers /usr/share/cmake*/SPIRV-Headers >/dev/null 2>&1; then
    fetch "https://github.com/KhronosGroup/SPIRV-Headers/archive/refs/tags/vulkan-sdk-1.4.363.0.tar.gz" spirv-headers.tar.gz
    rm -rf spirvh && mkdir spirvh && tar xzf spirv-headers.tar.gz -C spirvh --strip-components=1
    "$CMAKE" -S spirvh -B spirvh/build -DCMAKE_INSTALL_PREFIX="$WORK/spirv" -DSPIRV_HEADERS_ENABLE_TESTS=OFF >/dev/null
    "$CMAKE" --install spirvh/build >/dev/null
    OPTS+=(-DCMAKE_PREFIX_PATH="$WORK/spirv" -DCMAKE_CXX_FLAGS="-I$WORK/spirv/include" -DCMAKE_C_FLAGS="-I$WORK/spirv/include")
  fi
  OPTS+=(-DGGML_VULKAN=ON)
  GPU="Vulkan GPU + processor"
fi
echo "==> building whisper.cpp $WHISPER_VERSION ($GPU)"
rm -rf build
"$CMAKE" -S "whisper.cpp-$WHISPER_VERSION" -B build "${OPTS[@]}" >/dev/null
"$CMAKE" --build build --config Release -j"$(nproc)" --target whisper-cli
mkdir -p "$DEST"
rm -f "$DEST"/whisper-cli
cp build/bin/whisper-cli "$DEST/"
echo "==> installed $DEST/whisper-cli"
