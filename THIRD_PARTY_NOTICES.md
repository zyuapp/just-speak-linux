# Third-party notices

JustSpeak's original source code is licensed under the [MIT License](LICENSE).
Dependencies and downloaded models retain their own licenses. This document
records the principal native dependencies of the current local prototype; it
is not a complete inventory of every Rust or native dependency.

The current release executable statically includes eSpeak NG under
**GPL-3.0-or-later**, through sherpa-onnx's default TTS-enabled native runtime,
even though JustSpeak only uses speech recognition. This was confirmed in the
local executable by the presence of `espeak_Initialize`, `espeak_Cancel`, and
Piper phonemization symbols. The combined executable is **not MIT-only**.
The installation script and PKGBUILD currently support local prototype builds;
no binary release is published by this project.

## Native runtime

| Component | Version or source revision | Upstream license and notices |
| --- | --- | --- |
| sherpa-onnx, including its Rust bindings | 1.13.7 | [Apache-2.0](https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.7/LICENSE); [source](https://github.com/k2-fsa/sherpa-onnx/tree/v1.13.7) |
| ONNX Runtime | 1.27.1, as identified in the linked static runtime | [MIT](https://github.com/microsoft/onnxruntime/blob/v1.27.1/LICENSE), copyright Microsoft Corporation; [third-party notices](https://github.com/microsoft/onnxruntime/blob/v1.27.1/ThirdPartyNotices.txt); [source](https://github.com/microsoft/onnxruntime/tree/v1.27.1) |
| eSpeak NG, as selected by sherpa-onnx 1.13.7 | `csukuangfj/espeak-ng` revision `ed530aa113046142eb5115cf2fc9157854d0ffe1` | [GPL version 3 or later](https://github.com/espeak-ng/espeak-ng#license-information); [license text in selected source](https://github.com/csukuangfj/espeak-ng/blob/ed530aa113046142eb5115cf2fc9157854d0ffe1/COPYING); [source selection](https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.7/cmake/espeak-ng-for-piper.cmake) |
| Piper phonemize, as selected by sherpa-onnx 1.13.7 | `csukuangfj/piper-phonemize` revision `f3ff95afc03640bc1399e113e83361192a2fafb4` | [MIT](https://github.com/rhasspy/piper-phonemize/blob/master/LICENSE.md), copyright 2023 Michael Hansen; [selected source](https://github.com/csukuangfj/piper-phonemize/tree/f3ff95afc03640bc1399e113e83361192a2fafb4); [source selection](https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.7/cmake/piper-phonemize.cmake) |

The linked upstream texts define their respective terms. Apache-2.0 section 4
requires preservation of its license and applicable notices upon redistribution.
MIT requires preservation of its copyright and permission notices. GPLv3
sections 4–6 set the terms for conveying source and object code, including
corresponding-source requirements for object code. This summary and the source
links do not replace those texts or constitute a complete binary distribution
package.

Sherpa-onnx's
[CMake configuration](https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.7/CMakeLists.txt)
provides `SHERPA_ONNX_ENABLE_TTS=OFF` for a native build without its TTS
dependencies. JustSpeak currently uses the upstream prebuilt runtime with TTS
enabled. Its Rust dependency also names the TTS static libraries explicitly in
the [linker configuration](https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.7/sherpa-onnx/rust/sherpa-onnx-sys/build.rs).

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
