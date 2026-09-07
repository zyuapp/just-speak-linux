# CUDA benchmark on the development machine

Measured on 2026-09-06 with the same Parakeet TDT 0.6B v2 model family as the
CPU prototype. **The tested CUDA configuration did not meet the 20× target.**
The application remains on the verified CPU INT8 backend; CUDA support is
not shipped by this prototype.

## Hardware and method

- NVIDIA GeForce GTX 1650, 4096 MiB VRAM, driver 610.57.04.
- Approximately 3720 MiB VRAM free before the experiment.
- GPU computation and `nvidia-smi` monitoring ran outside the sandbox, where
  access to the installed NVIDIA driver worked.
- A small C++ program used the official sherpa-onnx 1.13.7 C API, created one
  recognizer, and decoded each WAV repeatedly using fresh streams. This uses
  the same native recognizer API as the Rust application.
- Greedy decoding, provider `cuda`, `CUDA_MODULE_LOADING=LAZY`, and either six
  or two host inference threads. All CUDA libraries lived in temporary
  directories; no system packages or desktop settings were changed.
- The short input was the upstream `test_wavs/0.wav`, 7.435 seconds. The long
  input was eight copies of that speech fixture, 59.480 seconds. Repetition
  provides a reproducible timing workload, not an accuracy benchmark for
  natural minute-long dictation.
- Timings include WAV loading, stream creation, feature processing, decoding,
  and result retrieval. Model loading is measured separately. They do not
  include microphone capture or clipboard/paste latency.
- “First inference” includes initial CUDA kernel/allocation overhead after
  model loading. Later runs reuse the resident recognizer. No batches or
  concurrent inference calls were used.

## Results

| Input | Host threads | Model load | First inference | Later resident inferences | Resident speed |
| --- | ---: | ---: | ---: | --- | --- |
| 59.480 s repeated speech | 6 | 3.685 s | 4.591 s | 4.274, 4.225, 4.232 s | 13.92–14.08× |
| 59.480 s repeated speech | 2 | 3.331 s | 4.562 s | 4.255, 4.224 s | 13.98–14.08× |
| 7.435 s speech fixture | 2 | 2.228 s | 0.779 s | 0.633, 0.632 s | 11.74–11.77× |

For the 59.480-second input, 20× requires inference within 2.974 seconds.
The fastest measured resident CUDA run took 4.224 seconds. Reducing the host
thread count did not materially improve the result.

During the six-thread run, `nvidia-smi` sampled approximately every 300 ms
reported **100% peak GPU utilization** and **2246 MiB peak total GPU memory
used**. The memory figure includes the desktop and other existing GPU use;
it is not an isolated per-process allocation measurement. The process exited
successfully, and its logs contained no provider-fallback or CUDA-loading
failure messages. The GPU was used, rather than the test merely selecting a
provider name and silently executing entirely on the CPU.

The FP16 model emitted **five trailing `<unk>` tokens** on each tested
minute-long transcription. This artifact was absent from the CPU INT8
baseline. These tokens were retained in the benchmark output and were not
silently cleaned up. None of the three short-clip runs emitted `<unk>` tokens.

## Exact model and runtime

The model was the official sherpa-onnx asset
`sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-fp16.tar.bz2`, containing
`encoder.fp16.onnx`, `decoder.fp16.onnx`, `joiner.fp16.onnx`, and `tokens.txt`.
The archive is 1,120,982,957 bytes. Its SHA256, computed after fetching it
directly from the official release, is:

```text
37f67a1a6c942dae27d345ee395fbd19e25ee48996faf70fca25779026054cf0
```

GitHub's asset metadata did not publish a digest for this FP16 archive, so this
is a locally measured artifact identifier, not verification against a
publisher-provided checksum.
[Model download](https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-fp16.tar.bz2).

The native runtime was
`sherpa-onnx-v1.13.7-cuda-12.x-cudnn-9.x-onnxruntime1.27.1-linux-x64-gpu.tar.bz2`,
239,093,227 bytes. It contains the shared sherpa C API, ONNX Runtime 1.27.1,
and the CUDA execution provider. Its SHA256 matched GitHub's published digest:

```text
5fb81446a46aedafb9c6ae707aeb21b8907eb7747ddc05af60ae3abb8a56b18c
```

[Runtime release](https://github.com/k2-fsa/sherpa-onnx/releases/tag/v1.13.7).

The following NVIDIA wheels were downloaded from PyPI, checked against their
published SHA256 digests, and unpacked into an isolated directory:

| Package | Version | Compressed bytes |
| --- | --- | ---: |
| `nvidia-cudnn-cu12` | 9.5.1.17 | 570,988,386 |
| `nvidia-cublas-cu12` | 12.6.4.1 | 393,138,322 |
| `nvidia-cuda-runtime-cu12` | 12.6.77 | 897,690 |
| `nvidia-cufft-cu12` | 11.3.0.4 | 200,221,632 |
| `nvidia-curand-cu12` | 10.3.7.77 | 56,279,010 |
| `nvidia-cuda-nvrtc-cu12` | 12.6.77 | 23,650,380 |

The additional downloads totaled approximately 2.60 GB compressed, including
the model and sherpa runtime. This setup follows the runtime's CUDA 12/cuDNN 9
major-version requirements; see the
[ONNX Runtime CUDA provider documentation](https://onnxruntime.ai/docs/execution-providers/CUDA-ExecutionProvider.html).

## Scope of the conclusion

This result applies to the tested GTX 1650, model export, and runtime versions.
It does not establish that every CUDA implementation or GPU would miss 20×.
The current result gives no reason to replace the faster measured CPU INT8
configuration with this CUDA configuration.

Sherpa-onnx's
[Parakeet native model implementation](https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.7/sherpa-onnx/csrc/offline-transducer-nemo-model.cc)
constructs the encoder, decoder, and joiner sessions using the same provider
options. It exposes no separate encoder-only CUDA setting in this path.
Evaluating a CUDA encoder with CPU decoder/joiner would require a native
change and rebuild; that was outside this bounded benchmark. No such
performance improvement is claimed here.
