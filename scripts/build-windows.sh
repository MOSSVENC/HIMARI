#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
export PATH="$HOME/.cargo/bin:$PATH"
command -v lld-link >/dev/null || { echo 'lld-link is required in WSL' >&2; exit 1; }
command -v llvm-rc >/dev/null || { echo 'llvm-rc is required in WSL' >&2; exit 1; }
# Use existing Windows SDK and CRT libraries read-only. Generated links stay in target/.
export RAM_MSVC_LIBDIR="$PWD/target/toolchains/msvc"
python3 - <<'PY'
import os
from pathlib import Path
sdk=Path(os.environ.get('RAM_WINDOWS_SDK', '/mnt/c/Program Files (x86)/Windows Kits/10/Lib'))
vc=Path(os.environ.get('RAM_WINDOWS_VC', '/mnt/c/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools/VC/Tools/MSVC'))
def newest(root):
    versions=sorted((p for p in root.iterdir() if p.is_dir() and p.name[0].isdigit()),key=lambda p: tuple(int(n) for n in p.name.split('.')))
    if not versions: raise SystemExit(f'Windows library versions missing: {root}')
    return versions[-1]
roots=[newest(sdk)/'um/x64',newest(sdk)/'ucrt/x64',newest(vc)/'lib/x64']
dst=Path(os.environ['RAM_MSVC_LIBDIR']);dst.mkdir(parents=True,exist_ok=True)
for root in roots:
    if not root.is_dir(): raise SystemExit(f'Existing Windows libraries missing: {root}')
    for source in root.iterdir():
        if source.suffix.lower() != '.lib': continue
        for name in (source.name.lower(),source.stem.upper()+'.lib',source.name.upper()):
            link=dst/name
            if link.is_symlink() and link.resolve() != source: link.unlink()
            if not link.exists(): link.symlink_to(source)
PY
export LIB="$RAM_MSVC_LIBDIR"
export CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER=lld-link
if [[ -z "${SKIA_BINARIES_URL:-}" ]]; then
    skia_archive=$(find "$PWD/target/x86_64-pc-windows-msvc" -path '*/.cache/skia-binaries-*-x86_64-pc-windows-msvc-d3d-gl-jpegd-jpege-pdf-svg-textlayout-vulkan.tar.gz' -type f -print -quit)
    if [[ -z "$skia_archive" ]]; then
        skia_name=skia-binaries-a25a0fdb7d90429aa2d1-x86_64-pc-windows-msvc-d3d-gl-jpegd-jpege-pdf-svg-textlayout-vulkan.tar.gz
        skia_archive="$PWD/target/toolchains/skia/$skia_name"
        mkdir -p -- "$(dirname -- "$skia_archive")"
        if [[ ! -f "$skia_archive" ]]; then
            trap 'rm -f -- "$skia_archive.partial"' EXIT
            curl --fail --location --connect-timeout 10 --max-time 120 --retry 2 \
                --output "$skia_archive.partial" \
                "https://github.com/rust-skia/skia-binaries/releases/download/0.99.0/$skia_name"
            mv -- "$skia_archive.partial" "$skia_archive"
            trap - EXIT
        fi
    fi
    printf '%s  %s\n' c410ec64ed356b11ba877847889b279daf52e6972ada79137493931668c12b79 "$skia_archive" | sha256sum --check --status
    export SKIA_BINARIES_URL="file://$skia_archive"
fi
# Use the published binary cache for this workspace's pinned Skia features.
unset FORCE_SKIA_BINARIES_DOWNLOAD FORCE_SKIA_BUILD
cargo build --locked --release --target x86_64-pc-windows-msvc --workspace "$@"
