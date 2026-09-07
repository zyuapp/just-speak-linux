# Third-party notices

JustSpeak's original source code is licensed under the [MIT License](LICENSE).
Native libraries, Rust dependencies, and downloaded models retain their own
licenses. Binary distributions must include this document and the generated
`licenses/` directory; a source license alone does not describe the executable.

## Native recognition runtime

`scripts/build-runtime.sh` builds sherpa-onnx 1.13.7 for CPU recognition with
`SHERPA_ONNX_ENABLE_TTS=OFF`. It also disables GPU support, speaker diarization,
PortAudio, WebSocket support, examples, and runtime executables. JustSpeak's
vendored Rust linker requires this runtime explicitly and does not download the
upstream TTS-enabled prebuilt archive. The changed linker is documented in
[vendor/sherpa-onnx-sys/JUSTSPEAK_PATCH.md](vendor/sherpa-onnx-sys/JUSTSPEAK_PATCH.md);
its FFI declarations and upstream license are unchanged.

All native C++ is compiled with `_GLIBCXX_USE_CXX11_ABI=0` to match the pinned
ONNX Runtime archive. Mixing its old string ABI with a host compiler's default
new ABI can abort in shared `std::regex` internals during runtime startup.

The build verifies the source archive and upstream dependency hashes, rejects
TTS libraries, and checks defined symbols for eSpeak NG and Piper. The resulting
runtime excludes eSpeak NG, Piper phonemize, and ucd. Earlier local prototype
executables used the upstream TTS-enabled archive and contained GPL-3.0-or-later
eSpeak NG; those executables are not the runtime used for public releases.

| Component | Version or revision | License and upstream source |
| --- | --- | --- |
| sherpa-onnx and Rust bindings | 1.13.7 | [Apache-2.0](https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.7/LICENSE) |
| ONNX Runtime | 1.27.1 | [MIT](https://github.com/microsoft/onnxruntime/blob/v1.27.1/LICENSE), Microsoft Corporation; [third-party notices](https://github.com/microsoft/onnxruntime/blob/v1.27.1/ThirdPartyNotices.txt) |
| kaldi-decoder | 0.3.0 | [Apache-2.0](https://github.com/k2-fsa/kaldi-decoder/tree/v0.3.0) |
| kaldifst | 1.8.0 | [Apache-2.0](https://github.com/k2-fsa/kaldifst/tree/v1.8.0); embedded libc++ basic-filebuf has MIT / University of Illinois notices |
| OpenFst | 1.8.5-2026-07-09 | [Apache-2.0](https://github.com/csukuangfj/openfst/tree/v1.8.5-2026-07-09), Google LLC |
| kaldi-native-fbank | 1.22.3 | [Apache-2.0](https://github.com/csukuangfj/kaldi-native-fbank/tree/v1.22.3) |
| KISS FFT | febd4caeed32e33ad8b2e0bb5ea77542c40f18ec | [BSD-3-Clause](https://github.com/mborgerding/kissfft/tree/febd4caeed32e33ad8b2e0bb5ea77542c40f18ec), Mark Borgerding |
| simple-sentencepiece | 0.7 | [Apache-2.0](https://github.com/pkufool/simple-sentencepiece/tree/v0.7); embedded Darts clone (BSD-2-Clause, Susumu Yata) and ThreadPool (zlib, Jakob Progsch and Václav Zeman) |
| Eigen | 5.0.1 | [MPL-2.0 and compatible third-party notices](https://gitlab.com/libeigen/eigen/-/tree/5.0.1) |
| nlohmann JSON | 3.12.0 | [MIT](https://github.com/nlohmann/json/blob/v3.12.0/LICENSE.MIT), Niels Lohmann |

The build copies each component's license and available notices into
`OUTPUT_DIRECTORY/licenses/native`, including notices embedded in third-party
headers and ONNX Runtime's complete `ThirdPartyNotices.txt`. It also includes
an archive of the exact Eigen source used in the build, preserving the source
and notices for this MPL-2.0 header library. `inventory.json` lists the collected
texts. The native components' own license terms remain authoritative.

The pinned sherpa-onnx source archive is
`https://codeload.github.com/k2-fsa/sherpa-onnx/tar.gz/refs/tags/v1.13.7`, SHA256
`ee0c20cafb34cc1f86afb2845babd941c26e46de4a9925cbe86fd55ff3557818`.
Its CMake files pin the transitive native dependencies. The prebuilt CPU ONNX
Runtime archive selected by that source is version 1.27.1, SHA256
`6b4df7fc46d3367b6be73fdea80dee323b9dc9eaa8dc50136a33d8524e7f06bb`.

## Downloaded recognition model

**Parakeet TDT 0.6B v2** is a model by **NVIDIA**, released under
[Creative Commons Attribution 4.0 International](https://creativecommons.org/licenses/by/4.0/).
The [NVIDIA model card](https://huggingface.co/nvidia/parakeet-tdt-0.6b-v2)
identifies its authorship and governing license.

JustSpeak downloads the sherpa-onnx project's **ONNX conversion with INT8
quantization**, distributed as
`sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8.tar.bz2`.
Conversion and quantization are changes from NVIDIA's original checkpoint;
JustSpeak does not modify the downloaded model files. See the
[sherpa-onnx model documentation](https://k2-fsa.github.io/sherpa/onnx/pretrained_models/offline-transducer/nemo-transducer-models.html#sherpa-onnx-nemo-parakeet-tdt-0-6b-v2-int8-english)
and [official release asset metadata](https://api.github.com/repos/k2-fsa/sherpa-onnx/releases/assets/283097678).
The download script verifies the asset's published SHA256:

```text
157c157bc51155e03e37d2466522a3a737dd9c72bb25f36eb18912964161e1ad
```

CC-BY-4.0 calls for attribution, a license link, and identification of changes
when sharing the licensed material. No NVIDIA or sherpa-onnx endorsement is
implied. Model weights and upstream test audio are downloaded separately; they
are not included in this repository or the local Arch package.

## Other dependencies

`Cargo.lock` records the resolved Rust crate versions. Their manifests and
license files, the native projects' dependency metadata, and ONNX Runtime's
third-party notices retain the authoritative component-level information.
PipeWire, Hyprland, wl-clipboard, Quickshell, and systemd are external installed
programs and are not copied into the JustSpeak package.
