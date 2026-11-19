#!/usr/bin/env bash
set -euo pipefail

project_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
ndk="${ANDROID_NDK_HOME:-${NDK_HOME:-${ANDROID_NDK_ROOT:-}}}"
if [[ -z "$ndk" ]]; then
    ndk="$(find "${ANDROID_HOME:?Android SDK or NDK is required}/ndk" -mindepth 1 -maxdepth 1 -type d | sort -V | tail -1)"
fi
toolchain="$ndk/toolchains/llvm/prebuilt/linux-x86_64/bin"
compiler="$toolchain/aarch64-linux-android24-clang"
test -x "$compiler"
destination="${1:-$project_dir/src-tauri/gen/android/app/src/main/jniLibs/arm64-v8a}"
build_dir="${ANDROID_PRORES_BUILD_DIR:-$(mktemp -d)}"
mkdir -p "$destination" "$build_dir"
destination="$(realpath "$destination")"
build_dir="$(realpath "$build_dir")"
cd "$build_dir"
version=7.1.4
source_archive="ffmpeg-$version.tar.xz"
source_sha256=71f4aac3573ed9060489cb62526a6c7dda815ae10993789611acd7be9fa9fbf4
if [[ ! -f "$source_archive" ]]; then
    curl --fail --location --retry 3 "https://ffmpeg.org/releases/$source_archive" -o "$source_archive.tmp"
    mv "$source_archive.tmp" "$source_archive"
fi
sha256sum --check <<< "$source_sha256  $source_archive"
tar -xf "$source_archive"
mkdir -p build
cd build
"../ffmpeg-$version/configure" \
    --cc="$compiler" --ar="$toolchain/llvm-ar" --ranlib="$toolchain/llvm-ranlib" \
    --strip="$toolchain/llvm-strip" --arch=aarch64 --target-os=android --enable-cross-compile \
    --enable-pic --enable-small --disable-shared --enable-static \
    --disable-programs --disable-doc --disable-network --disable-autodetect \
    --disable-avdevice --disable-avfilter --disable-swscale --disable-swresample \
    --disable-everything --enable-encoder=prores_ks --enable-muxer=mov --enable-protocol=file
make -j "${ANDROID_PRORES_JOBS:-$(nproc)}" libavformat/libavformat.a libavcodec/libavcodec.a libavutil/libavutil.a
"$compiler" -O2 -fPIC -fvisibility=hidden -shared \
    -Wall -Wextra -Werror -I "../ffmpeg-$version" -I . \
    -o "$destination/libphitkc_prores.so" "$project_dir/src-tauri/src/android_prores.c" \
    -Wl,--exclude-libs,ALL -Wl,-z,defs -Wl,-z,max-page-size=16384 -Wl,-soname,libphitkc_prores.so \
    libavformat/libavformat.a libavcodec/libavcodec.a libavutil/libavutil.a -lm
"$toolchain/llvm-strip" --strip-unneeded "$destination/libphitkc_prores.so"
