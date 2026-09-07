#!/usr/bin/env bash
# Build the pinned CPU runtime without TTS, in a caller-owned directory.
set -euo pipefail

if [[ $# != 1 || $1 == --help ]]; then
  echo "Usage: $0 OUTPUT_DIRECTORY" >&2
  echo "Then: SHERPA_ONNX_LIB_DIR=OUTPUT_DIRECTORY/lib cargo build --release --locked" >&2
  exit 2
fi
[[ $(uname -s) == Linux && $(uname -m) == x86_64 ]] || {
  echo "This runtime build supports Linux x86_64 only." >&2; exit 1;
}
for tool in cmake make curl tar sha256sum nm python3; do
  command -v "$tool" >/dev/null || { echo "Required build tool: $tool" >&2; exit 1; }
done
mkdir -p "$1"
prefix=$(cd "$1" && pwd)
# A failed rebuild must not leave a previous verification marker usable.
rm -f "$prefix/lib/just-speak-asr-runtime.txt"
work=${JUST_SPEAK_NATIVE_WORK:-"$prefix/.build"}
mkdir -p "$work"
work=$(cd "$work" && pwd)
source_dir="$work/sherpa-onnx-1.13.7"
build_dir="$work/build"
jobs=${CMAKE_BUILD_PARALLEL_LEVEL:-2}
[[ $jobs =~ ^[1-9][0-9]*$ ]] || { echo "Invalid CMAKE_BUILD_PARALLEL_LEVEL" >&2; exit 1; }

download() {
  local url=$1 file=$2 hash=$3
  if [[ ! -f $file ]]; then
    curl --fail --location --retry 3 --connect-timeout 20 --max-time 600 "$url" -o "$file.part"
    mv "$file.part" "$file"
  fi
  printf '%s  %s\n' "$hash" "$file" | sha256sum --check --status || {
    echo "Checksum mismatch: $file" >&2; exit 1;
  }
}

download https://codeload.github.com/k2-fsa/sherpa-onnx/tar.gz/refs/tags/v1.13.7 \
  "$work/sherpa-onnx-v1.13.7.tar.gz" \
  ee0c20cafb34cc1f86afb2845babd941c26e46de4a9925cbe86fd55ff3557818
if [[ ! -d $source_dir ]]; then
  tar -xzf "$work/sherpa-onnx-v1.13.7.tar.gz" -C "$work"
fi

# Upstream CMake also pins each native dependency with a SHA256 hash. Disable
# host ONNX Runtime discovery so this build uses exactly that dependency set.
# The pinned glibc2_17 ORT archive uses libstdc++'s old string ABI. All native
# C++ must match it: mixed ABIs collide in untagged std::regex internals and
# abort during ORT device discovery, even though the public interface is C.
cmake -S "$source_dir" -B "$build_dir" \
  -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX="$prefix" \
  -DCMAKE_CXX_FLAGS="${CXXFLAGS:-} -D_GLIBCXX_USE_CXX11_ABI=0" \
  -DBUILD_SHARED_LIBS=OFF -DSHERPA_ONNX_ENABLE_C_API=ON \
  -DSHERPA_ONNX_ENABLE_TTS=OFF -DSHERPA_ONNX_ENABLE_SPEAKER_DIARIZATION=OFF \
  -DSHERPA_ONNX_ENABLE_GPU=OFF -DSHERPA_ONNX_ENABLE_PYTHON=OFF \
  -DSHERPA_ONNX_ENABLE_TESTS=OFF -DSHERPA_ONNX_ENABLE_PORTAUDIO=OFF \
  -DSHERPA_ONNX_ENABLE_WEBSOCKET=OFF -DSHERPA_ONNX_ENABLE_BINARY=OFF \
  -DSHERPA_ONNX_BUILD_C_API_EXAMPLES=OFF \
  -DSHERPA_ONNX_LINK_LIBSTDCPP_STATICALLY=OFF \
  -DSHERPA_ONNX_USE_PRE_INSTALLED_ONNXRUNTIME_IF_AVAILABLE=OFF
cmake --build "$build_dir" --parallel "$jobs"
cmake --install "$build_dir"

for library in piper_phonemize espeak-ng ucd; do
  [[ ! -e "$prefix/lib/lib$library.a" ]] || {
    echo "Unexpected TTS archive; use a clean output directory: $library" >&2; exit 1;
  }
done
nm -C --defined-only "$prefix"/lib/*.a > "$work/native-symbols.txt" 2> "$work/nm.log"
if python3 - "$work/native-symbols.txt" <<'PY'
import re, sys
from pathlib import Path
symbols = Path(sys.argv[1]).read_text(errors="replace")
sys.exit(0 if re.search(r"espeak[_:]|piper::|piper_phonemize", symbols, re.I) else 1)
PY
then
  echo "TTS symbols found in the recognition runtime; refusing to mark it verified." >&2
  exit 1
fi
python3 - "$work/native-symbols.txt" <<'PY'
import sys
from pathlib import Path
if "std::__cxx11::basic_string" in Path(sys.argv[1]).read_text(errors="replace"):
    raise SystemExit("Native C++ string ABI does not match the pinned ONNX Runtime archive.")
PY

# The ORT binary archive omits these texts; get them from its exact source tag.
mkdir -p "$work/notices"
download https://raw.githubusercontent.com/microsoft/onnxruntime/v1.27.1/LICENSE \
  "$work/notices/onnxruntime-LICENSE" \
  2f07c72751aed99790b8a4869cf2311df85a860b22ded05fa22803587a48922c
download https://raw.githubusercontent.com/microsoft/onnxruntime/v1.27.1/ThirdPartyNotices.txt \
  "$work/notices/onnxruntime-ThirdPartyNotices.txt" \
  0e07b95f3a8d6230037707c5c4a2b554d12c4cb67369669ac255635528ffcee2

python3 - "$source_dir" "$build_dir" "$prefix" "$work/notices" <<'PY'
import json, re, shutil, sys, tarfile
from pathlib import Path
source, build, prefix, notices = map(Path, sys.argv[1:])
destination = prefix / "licenses" / "native"
destination.mkdir(parents=True, exist_ok=True)
components = {"sherpa-onnx": source}
components.update({p.name.removesuffix("-src"): p for p in (build / "_deps").glob("*-src")})
inventory = []
for name, directory in sorted(components.items()):
    target = destination / name
    target.mkdir(exist_ok=True)
    files = [p for p in directory.iterdir() if p.is_file() and
             p.name.upper().startswith(("LICENSE", "COPYING", "NOTICE", "AUTHORS"))]
    if name == "kissfft":
        files += [directory / "LICENSES" / "BSD-3-Clause", directory / "LICENSES" / "Unlicense"]
    if name == "simple-sentencepiece":
        files += [directory / "ssentencepiece/csrc/darts.h",
                  directory / "ssentencepiece/csrc/threadpool.h"]
    if name == "kaldifst":
        files += [directory / "kaldifst/csrc/basic-filebuf.h"]
    if name == "onnxruntime":
        files = list(notices.glob("onnxruntime-*"))
    if not files:
        raise SystemExit(f"No license texts found for native component: {name}")
    for path in files:
        shutil.copyfile(path, target / path.name)
    inventory.append({"component": name, "license_files": sorted(p.name for p in files)})
    if name == "eigen":
        # Include exact source for this MPL-2.0 header library alongside notices.
        with tarfile.open(target / "eigen-5.0.1-source.tar.gz", "w:gz") as archive:
            archive.add(directory, arcname="eigen-5.0.1")
(destination / "inventory.json").write_text(json.dumps(inventory, indent=2) + "\n")
shutil.copyfile(source / "LICENSE", destination / "Apache-2.0.txt")
options = [line for line in (build / "CMakeCache.txt").read_text().splitlines()
           if re.match(r"(?:SHERPA_ONNX_[A-Z_]+:BOOL|BUILD_SHARED_LIBS:BOOL|CMAKE_BUILD_TYPE:STRING|CMAKE_CXX_FLAGS:STRING)=", line)]
(destination / "build-options.txt").write_text("\n".join(options) + "\n")
PY

printf 'sherpa-onnx=1.13.7\nonnxruntime=1.27.1\ntts=off\nprovider=cpu\ncxx11_abi=0\n' \
  > "$prefix/lib/just-speak-asr-runtime.txt"
echo "ASR-only runtime verified. Build with:"
printf 'SHERPA_ONNX_LIB_DIR=%q cargo build --release --locked\n' "$prefix/lib"
echo "Redistribution notices: $prefix/licenses/native"
