//! Two jobs, both only when `local-stt` is on.
//!
//! Check the sherpa-onnx prebuilt's two dylibs against pinned SHA-256s.
//! `sherpa-onnx-sys`'s build script downloads them from GitHub without checking a hash, and it runs first because it is a direct dependency with `links`.
//! They ship in the release, so different bytes must fail the build.
//! The pins are the dylibs as unpacked from the `shared-lib` release asset whose GitHub digest is in the comment above each arch.
//! The unpacked dylibs are checked rather than the archive because CI's rust-cache keeps the folder and drops the archive.
//!
//! Link the Clang runtime library for whisper.cpp's Metal backend.
//! `ggml-metal.m` guards macOS 14+ APIs with `@available`, and with a macOS 13 deployment target Clang lowers each guard to `__isPlatformVersionAtLeast`.
//! That symbol lives in `libclang_rt.osx.a`, which neither rustc's `-nodefaultlibs` link nor `compiler_builtins` provides.

use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

type Pin = (&'static str, &'static str);

/// Per target arch: the unpacked `shared-lib` folder at the `sherpa-onnx-sys` version Cargo.toml pins, then each dylib and its SHA-256.
const SHERPA_ONNX_DYLIBS: &[(&str, &str, [Pin; 2])] = &[
    // sherpa-onnx-v2.0.1-osx-arm64-shared-lib.tar.bz2, sha256:e2d57ad9c83803d6c48f259caa66e44879bd4c4724c61f0d51a277b411bffb1d
    (
        "aarch64",
        "sherpa-onnx-v2.0.1-osx-arm64-shared-lib",
        [
            (
                "libsherpa-onnx-c-api.dylib",
                "c22f53f3c16c077e0ce0f31eca4a6637c86bf842bed67769d6d5a99ebafab340",
            ),
            (
                "libonnxruntime.dylib",
                "3567d114f7299d559993e536d605a6f46d7bc9d2542004accc80ee9bf5457f0b",
            ),
        ],
    ),
    // sherpa-onnx-v2.0.1-osx-x64-shared-lib.tar.bz2, sha256:be9042971c5d1b14da76a960534d2a8cdb6f0e5e02f5e1dc8bddff8576359bdd
    (
        "x86_64",
        "sherpa-onnx-v2.0.1-osx-x64-shared-lib",
        [
            (
                "libsherpa-onnx-c-api.dylib",
                "0cd8c47231164215b8813f77d6b28bf61bfd2bbf9f1580f758c063c05c30ce98",
            ),
            (
                "libonnxruntime.dylib",
                "2bf27180c09e6fd3f407c9ab25205ebf1ff95f2d53bfa35e35409b5849e0ac70",
            ),
        ],
    ),
];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=CC");

    if std::env::var_os("CARGO_FEATURE_LOCAL_STT").is_none() {
        return;
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }

    check_sherpa_onnx_dylibs();
    link_clang_runtime();
}

fn check_sherpa_onnx_dylibs() {
    println!("cargo:rerun-if-env-changed=SHERPA_ONNX_LIB_DIR");
    // Libraries from elsewhere are not the pinned ones.
    if std::env::var_os("SHERPA_ONNX_LIB_DIR").is_some() {
        panic!("SHERPA_ONNX_LIB_DIR skips the pinned sherpa-onnx prebuilt; unset it, or use SHERPA_ONNX_ARCHIVE_DIR");
    }
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap();
    let (_, stem, dylibs) = SHERPA_ONNX_DYLIBS
        .iter()
        .find(|(a, _, _)| *a == arch)
        .unwrap_or_else(|| panic!("no pinned sherpa-onnx prebuilt for {arch}"));
    // Where sherpa-onnx-sys unpacks it: `<target dir>/sherpa-onnx-prebuilt/<stem>/lib`, found the way its build script finds it.
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let target_dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            out_dir
                .ancestors()
                .find(|p| p.file_name() == Some("target".as_ref()))
                .map(Path::to_path_buf)
        })
        .unwrap_or(out_dir);
    let lib = target_dir
        .join("sherpa-onnx-prebuilt")
        .join(stem)
        .join("lib");
    for (name, pinned) in dylibs {
        let path = lib.join(name);
        println!("cargo:rerun-if-changed={}", path.display());
        let bytes = std::fs::read(&path).unwrap_or_else(|e| {
            panic!(
                "{}: {e}. sherpa-onnx-sys unpacks its download there; `cargo clean -p sherpa-onnx-sys` fetches it again",
                path.display()
            )
        });
        let actual = format!("{:x}", Sha256::digest(&bytes));
        if actual != *pinned {
            panic!(
                "{} has SHA-256 {actual}, but {pinned} is pinned. Delete {} and rebuild",
                path.display(),
                target_dir.join("sherpa-onnx-prebuilt").display()
            );
        }
    }
}

fn link_clang_runtime() {
    let Some(runtime_dir) = clang_runtime_dir() else {
        println!(
            "cargo:warning=could not resolve the Clang runtime directory; the link may fail \
             with an undefined `___isPlatformVersionAtLeast` from whisper.cpp's Metal backend"
        );
        return;
    };

    if !runtime_dir.join("libclang_rt.osx.a").is_file() {
        println!(
            "cargo:warning=libclang_rt.osx.a not found in {}; check that the Xcode command line \
             tools are installed and `xcode-select -p` points at the toolchain you build with",
            runtime_dir.display()
        );
        return;
    }

    println!("cargo:rustc-link-search=native={}", runtime_dir.display());
    // `-bundle` keeps the archive on the final link line instead of copying its members into our rlib,
    // where its `compiler_rt` builtins would duplicate the ones `compiler_builtins` already provides.
    println!("cargo:rustc-link-lib=static:-bundle=clang_rt.osx");
}

/// Ask the C compiler (honouring `CC`) where its runtime libraries live.
fn clang_runtime_dir() -> Option<PathBuf> {
    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".to_string());
    let output = Command::new(cc).arg("--print-runtime-dir").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let path = PathBuf::from(String::from_utf8(output.stdout).ok()?.trim());
    path.is_dir().then_some(path)
}
