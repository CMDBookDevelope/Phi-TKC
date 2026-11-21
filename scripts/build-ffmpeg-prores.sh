#!/usr/bin/env bash
# Rebuild the separate LGPL FFmpeg ProRes executable from the adjacent sources.
# Windows CI uses MSYS2 MINGW64. No GPL/nonfree libraries are enabled; shader
# compilation is performed by build tools, not a linked libshaderc/glslang.
set -euo pipefail
if [ "$#" -ne 2 ]; then
  echo "Usage: $0 <FFmpeg-source-directory> <sidecar-output-directory>" >&2
  exit 2
fi
source_dir=$(realpath "$1")
output=$(realpath "$2")
script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
mkdir -p "$output/ffmpeg-prores-distribution" "$output/build"
cd "$output/build"
configure_args=(
  --disable-autodetect --disable-gpl --disable-nonfree --disable-version3
  --disable-doc --disable-debug --disable-ffplay --disable-ffprobe
  --disable-shared --enable-static --enable-ffmpeg --enable-vulkan --enable-libzimg
  --extra-version=Phi-TKC-ProRes --pkg-config-flags=--static
)
make_target=ffmpeg
case "$(uname -s)" in
  MINGW*|MSYS*)
    make_target=ffmpeg.exe
    configure_args+=(--target-os=mingw32 --arch=x86_64 --extra-ldflags=-static)
    ;;
  *)
    if [ -n "${PHI_FFMPEG_CROSS_PREFIX:-}" ]; then
      make_target=ffmpeg.exe
      configure_args+=(--enable-cross-compile --target-os=mingw32 --arch=x86_64
        "--cross-prefix=${PHI_FFMPEG_CROSS_PREFIX}" --extra-ldflags=-static)
    fi
    ;;
esac
if [ -n "${PHI_FFMPEG_GLSLC:-}" ]; then
  configure_args+=("--glslc=${PHI_FFMPEG_GLSLC}")
fi
"$source_dir/configure" "${configure_args[@]}"
# configure silently drops an encoder when a shader tool cannot compile its
# requirements. Fail before compiling, without requiring a Vulkan device.
python3 - <<'PY'
from pathlib import Path
config = Path('config_components.h').read_text()
for feature in ('PRORES_KS_VULKAN_ENCODER', 'PRORES_KS_ENCODER', 'ZSCALE_FILTER'):
    if f'#define CONFIG_{feature} 1' not in config:
        raise SystemExit(f'Required FFmpeg component disabled: {feature}')
PY
make -j"${PHI_FFMPEG_JOBS:-$(nproc)}" "$make_target"
if [ -f ffmpeg.exe ]; then
  cp ffmpeg.exe "$output/ffmpeg-prores.exe"
else
  cp ffmpeg "$output/ffmpeg-prores.exe"
fi
distribution="$output/ffmpeg-prores-distribution"
cp config.h config_components.h "$distribution/"
cp ffbuild/config.mak "$distribution/config.mak"
# Keep zimg's installed license with the statically linked encoder.
# MSYS2 places it under share/doc/zimg/COPYING.
if [ "$make_target" = ffmpeg.exe ] && [ -n "${MINGW_PREFIX:-}" ]; then
  cp "$MINGW_PREFIX/share/doc/zimg/COPYING" "$distribution/zimg-LICENSE.txt"
  notice_root="$MINGW_PREFIX/share/licenses"
else
  test -f /usr/share/doc/libzimg2/copyright
  cp /usr/share/doc/libzimg2/copyright "$distribution/zimg-LICENSE.txt"
  notice_root=/usr/share/licenses
fi
for notice in "$notice_root"/gcc*/COPYING.RUNTIME "$notice_root"/gcc*/COPYING3 \
    "$notice_root"/gcc*/COPYING3.LIB "$notice_root"/vulkan-headers*/LICENSE*; do
  [ -f "$notice" ] || continue
  cp "$notice" "$distribution/$(basename "$(dirname "$notice")")-$(basename "$notice")"
done
{
  printf 'FFmpeg configuration:\n'
  printf '%q ' "$source_dir/configure" "${configure_args[@]}"
  printf '\n\nToolchain:\n'
  ${PHI_FFMPEG_CROSS_PREFIX:-}gcc --version
  make --version
  printf '\nzimg: '; pkg-config --modversion zimg
  if command -v pacman >/dev/null && [ -n "${MINGW_PACKAGE_PREFIX:-}" ]; then
    pacman -Q "${MINGW_PACKAGE_PREFIX}-vulkan-headers" "${MINGW_PACKAGE_PREFIX}-spirv-headers"
  else
    printf 'Vulkan headers: '; pkg-config --modversion vulkan
  fi
  printf '\nShader compiler:\n'
  if [ -n "${PHI_FFMPEG_GLSLC:-}" ]; then
    "$PHI_FFMPEG_GLSLC" --version
  elif command -v glslc >/dev/null; then
    glslc --version
  elif command -v glslangValidator >/dev/null; then
    glslangValidator --version
  else
    glslang --version
  fi
} > "$distribution/build-info.txt"
# No build objects go in the installer. Only source/config/license/provenance
# and the executable are finalized by the Python helper after this script.
python3 "$script_dir/fetch-ffmpeg.py" --output "$output" --finalize
