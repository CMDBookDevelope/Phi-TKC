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
# zimg is the only external linked library: permissive MIT, not GPL/LGPL.
# Copy its installed copyright/license, and GCC's runtime exception notices.
python3 - "$distribution" <<'PY'
import os, shutil, sys
from pathlib import Path
out = Path(sys.argv[1])
prefix = Path(os.environ.get('MINGW_PREFIX', '/usr'))
roots = [prefix / 'share/licenses', Path('/usr/share/doc')]
zimg = [p for root in roots if root.exists() for pattern in ('zimg*/LICENSE*', 'libzimg*/copyright') for p in root.glob(pattern) if p.is_file()]
if not zimg:
    raise SystemExit('zimg copyright/license is required for binary redistribution')
shutil.copy2(zimg[0], out / 'zimg-LICENSE.txt')
for root in roots:
    if root.exists():
        for pattern in ('gcc*/COPYING.RUNTIME', 'gcc*/COPYING3', 'gcc*/COPYING3.LIB'):
            for p in root.glob(pattern):
                if p.is_file():
                    shutil.copy2(p, out / ('GCC-' + p.name))
# The Vulkan headers are permissively licensed and header-only.
for root in roots:
    for p in root.glob('vulkan-headers*/LICENSE*') if root.exists() else []:
        if p.is_file():
            shutil.copy2(p, out / ('Vulkan-Headers-' + p.name))
PY
{
  printf 'FFmpeg configuration:\n'
  printf '%q ' "$source_dir/configure" "${configure_args[@]}"
  printf '\n\nToolchain:\n'
  ${PHI_FFMPEG_CROSS_PREFIX:-}gcc --version
  make --version
  printf '\nzimg: '; pkg-config --modversion zimg
  printf 'Vulkan headers: '; pkg-config --modversion vulkan
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
