#!/usr/bin/env bash
# Cross-compile whisper.cpp's whisper-cli.exe for Windows x64 with llvm-mingw, for VeloMov's
# auto captions. Output: ~/.cache/velomov-cross/whisper-win/ (used by build-cross.sh).
# - No Visual C++ runtime needed (libc++ is linked in).
# - The processor backend comes in variants for every x64 CPU generation (picked at run time).
# - A Vulkan GPU backend (ggml-vulkan.dll) is included: used when the graphics driver provides
#   Vulkan, otherwise whisper falls back to the processor by itself.
# Needs: glslc (shaderc) and a host C++ compiler; CMake, headers and llvm-mingw are downloaded.
set -euo pipefail
X="${VELOMOV_CROSS:-$HOME/.cache/velomov-cross}"
W="$X/whisper-win-build"
OUT="$X/whisper-win"
WHISPER_VERSION="${WHISPER_VERSION:-1.9.5}"
LLVM_MINGW=llvm-mingw-20261006-ucrt-ubuntu-22.04-x86_64
VK_SDK=vulkan-sdk-1.4.363.0
mkdir -p "$X/dl" "$X/ext" "$W"
fetch() { [ -s "$X/dl/$2" ] || { echo "==> downloading $2"; curl -fL --retry 3 -o "$X/dl/$2.part" "$1" && mv "$X/dl/$2.part" "$X/dl/$2"; }; }

if [ ! -d "$X/ext/$LLVM_MINGW" ]; then
  fetch "https://github.com/mstorsjo/llvm-mingw/releases/download/20261006/$LLVM_MINGW.tar.xz" llvm-mingw.tar.xz
  tar xf "$X/dl/llvm-mingw.tar.xz" -C "$X/ext"
fi
MINGW="$X/ext/$LLVM_MINGW"
export PATH="$MINGW/bin:$PATH"
CMAKE=cmake
if ! command -v cmake >/dev/null; then
  fetch https://github.com/Kitware/CMake/releases/download/v4.4.4/cmake-4.4.4-linux-x86_64.tar.gz cmake.tar.gz
  [ -d "$X/ext/cmake-4.4.4-linux-x86_64" ] || tar xzf "$X/dl/cmake.tar.gz" -C "$X/ext"
  CMAKE="$X/ext/cmake-4.4.4-linux-x86_64/bin/cmake"
fi
command -v glslc >/dev/null || { echo "glslc (shaderc) is needed for the Vulkan shaders"; exit 1; }

fetch "https://github.com/ggml-org/whisper.cpp/archive/refs/tags/v$WHISPER_VERSION.tar.gz" whisper-src.tar.gz
fetch "https://github.com/KhronosGroup/Vulkan-Headers/archive/refs/tags/$VK_SDK.tar.gz" vkheaders.tar.gz
fetch "https://github.com/KhronosGroup/SPIRV-Headers/archive/refs/tags/$VK_SDK.tar.gz" spirv-headers.tar.gz
cd "$W"
rm -rf src vkh spirvh spirv && mkdir src vkh spirvh
tar xzf "$X/dl/whisper-src.tar.gz" -C src --strip-components=1
tar xzf "$X/dl/vkheaders.tar.gz" -C vkh --strip-components=1
tar xzf "$X/dl/spirv-headers.tar.gz" -C spirvh --strip-components=1
"$CMAKE" -S spirvh -B spirvh/build -DCMAKE_INSTALL_PREFIX="$W/spirv" -DSPIRV_HEADERS_ENABLE_TESTS=OFF >/dev/null
"$CMAKE" --install spirvh/build >/dev/null

# import library for vulkan-1.dll (the Vulkan loader every GPU driver installs)
{
  echo "LIBRARY vulkan-1.dll"
  echo "EXPORTS"
  grep -ho 'VKAPI_ATTR [A-Za-z0-9_]* VKAPI_CALL vk[A-Za-z0-9_]*' vkh/include/vulkan/vulkan_core.h vkh/include/vulkan/vulkan_win32.h \
    | awk '{print "  " $4}' | sort -u
} > vulkan-1.def
x86_64-w64-mingw32-dlltool -d vulkan-1.def -l libvulkan-1.a

cat > toolchain.cmake <<EOF
set(CMAKE_SYSTEM_NAME Windows)
set(CMAKE_SYSTEM_PROCESSOR x86_64)
set(CMAKE_C_COMPILER x86_64-w64-mingw32-clang)
set(CMAKE_CXX_COMPILER x86_64-w64-mingw32-clang++)
set(CMAKE_RC_COMPILER x86_64-w64-mingw32-windres)
set(CMAKE_FIND_ROOT_PATH "$MINGW/x86_64-w64-mingw32" "$W/spirv")
set(CMAKE_FIND_ROOT_PATH_MODE_PROGRAM NEVER)
set(CMAKE_FIND_ROOT_PATH_MODE_LIBRARY ONLY)
set(CMAKE_FIND_ROOT_PATH_MODE_INCLUDE BOTH)
set(CMAKE_FIND_ROOT_PATH_MODE_PACKAGE BOTH)
EOF
echo "==> building whisper.cpp $WHISPER_VERSION for Windows (CPU variants + Vulkan)"
"$CMAKE" -S src -B build -DCMAKE_TOOLCHAIN_FILE="$W/toolchain.cmake" -DCMAKE_BUILD_TYPE=Release \
  -DBUILD_SHARED_LIBS=ON -DGGML_BACKEND_DL=ON -DGGML_CPU_ALL_VARIANTS=ON -DGGML_NATIVE=OFF -DGGML_OPENMP=OFF \
  -DGGML_VULKAN=ON -DVulkan_INCLUDE_DIR="$W/vkh/include" -DVulkan_LIBRARY="$W/libvulkan-1.a" \
  -DVulkan_GLSLC_EXECUTABLE="$(command -v glslc)" -DCMAKE_PREFIX_PATH="$W/spirv" \
  -DCMAKE_C_FLAGS="-I$W/spirv/include" -DCMAKE_CXX_FLAGS="-I$W/spirv/include -include algorithm" \
  -DCMAKE_EXE_LINKER_FLAGS="-static" -DCMAKE_SHARED_LINKER_FLAGS="-static" \
  -DWHISPER_BUILD_TESTS=OFF -DWHISPER_BUILD_SERVER=OFF -DWHISPER_SDL2=OFF -DWHISPER_CURL=OFF >/dev/null
"$CMAKE" --build build --config Release -j"$(nproc)" --target whisper-cli
# the CPU and GPU backends are loaded at run time: build each of them
BACKENDS=$("$CMAKE" --build build --target help | grep -o 'ggml-cpu-[a-z0-9_]*\|ggml-vulkan' | sort -u)
"$CMAKE" --build build --config Release -j"$(nproc)" --target $BACKENDS
rm -rf "$OUT" && mkdir -p "$OUT"
cp build/bin/whisper-cli.exe "$OUT/"
find build/bin -maxdepth 1 -name '*.dll' -exec cp {} "$OUT/" \;
cp src/LICENSE "$OUT/LICENSE-whisper.cpp.txt"
echo "==> $OUT"; ls "$OUT"
