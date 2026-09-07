# JustSpeak native linker patch

The `src/` directory, `LICENSE`, and upstream `README.md` are copied unchanged
from the Apache-2.0 `sherpa-onnx-sys` 1.13.7 crates.io package.

JustSpeak replaces `build.rs` and its Cargo build dependencies. The replacement
supports Linux x86_64 GNU, statically links the speech-recognition runtime, and
requires an explicit `SHERPA_ONNX_LIB_DIR` produced by
`scripts/build-runtime.sh`. It does not download the upstream TTS-enabled
prebuilt archive. Piper phonemize, eSpeak NG, and ucd are removed from the link
list. FFI declarations for disabled APIs remain present but are not supported
by this runtime; JustSpeak uses the offline recognition APIs only.

The runtime uses `_GLIBCXX_USE_CXX11_ABI=0` throughout its C++ dependencies to
match the pinned prebuilt ONNX Runtime archive. The marker includes this ABI
selection; mismatched builds can crash inside `std::regex` during ORT startup.

The runtime marker is a build consistency check, not a signature. The build
script verifies archive checksums and checks the resulting native symbols
before creating the marker. Release packaging separately checks the final
executable and bundles license texts.
