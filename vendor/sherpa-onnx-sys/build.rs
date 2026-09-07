// JustSpeak modification, 2026: replace the upstream prebuilt downloader with
// explicit linking against our CPU runtime built with TTS disabled. The FFI
// declarations remain unchanged from sherpa-onnx-sys 1.13.7 (Apache-2.0).
use std::{env, fs, path::PathBuf};

const LIBRARIES: &[&str] = &[
    "sherpa-onnx-c-api",
    "sherpa-onnx-core",
    "kaldi-decoder-core",
    "sherpa-onnx-kaldifst-core",
    "sherpa-onnx-fstfar",
    "sherpa-onnx-fst",
    "kaldi-native-fbank-core",
    "kissfft-float",
    "onnxruntime",
    "ssentencepiece_core",
];

fn main() {
    println!("cargo:rerun-if-env-changed=SHERPA_ONNX_LIB_DIR");
    println!("cargo:rerun-if-env-changed=DOCS_RS");
    if env::var_os("DOCS_RS").is_some() {
        return;
    }
    assert_eq!(
        env::var("TARGET").unwrap(),
        "x86_64-unknown-linux-gnu",
        "JustSpeak's verified native runtime supports Linux x86_64 GNU only"
    );
    assert!(
        env::var_os("CARGO_FEATURE_SHARED").is_none(),
        "JustSpeak's ASR-only runtime requires static linking"
    );
    let directory = PathBuf::from(env::var_os("SHERPA_ONNX_LIB_DIR").expect(
        "Build the ASR-only runtime with scripts/build-runtime.sh /tmp/native, then set SHERPA_ONNX_LIB_DIR=/tmp/native/lib",
    ));
    let marker = directory.join("just-speak-asr-runtime.txt");
    println!("cargo:rerun-if-changed={}", marker.display());
    assert_eq!(
        fs::read_to_string(&marker).unwrap_or_default(),
        "sherpa-onnx=1.13.7\nonnxruntime=1.27.1\ntts=off\nprovider=cpu\ncxx11_abi=0\n",
        "SHERPA_ONNX_LIB_DIR must contain the runtime verified by scripts/build-runtime.sh"
    );
    for forbidden in ["piper_phonemize", "espeak-ng", "ucd"] {
        assert!(
            !directory.join(format!("lib{forbidden}.a")).exists(),
            "TTS library found in the ASR-only runtime directory: {forbidden}"
        );
    }
    println!("cargo:rustc-link-search=native={}", directory.display());
    for library in LIBRARIES {
        let archive = directory.join(format!("lib{library}.a"));
        assert!(
            archive.is_file(),
            "Missing ASR runtime archive: {}",
            archive.display()
        );
        println!("cargo:rerun-if-changed={}", archive.display());
        println!("cargo:rustc-link-lib=static={library}");
    }
    for library in ["stdc++", "m", "pthread", "dl"] {
        println!("cargo:rustc-link-lib={library}");
    }
}
