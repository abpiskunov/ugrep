//! Links the retained C codec libraries into the replay harness, so the Rust zstream port can call them through FFI
//! (`extern "C"` in rust/src_leaves). The libraries are built outside the repository by
//! migration/harness/zstream/fetch-codecs.ps1 (static, /MT, x64) into `<repo>\..\codec-build\lib`; CPP2RUST_CODEC_LIB
//! names another directory. A missing library is left out with a warning: the harness still builds, and fails to link
//! only once the port references a symbol of that library.
use std::path::PathBuf;

const LIBS: [&str; 6] = ["zlib", "bz2", "lzma", "lz4", "zstd", "viiz"];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=CPP2RUST_CODEC_LIB");
    let dir = match std::env::var_os("CPP2RUST_CODEC_LIB") {
        Some(d) => PathBuf::from(d),
        None => {
            let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
            // <repo>/rust/src_leaves-harness -> <repo>/../codec-build/lib
            manifest.join("..").join("..").join("..").join("codec-build").join("lib")
        }
    };
    if !dir.is_dir() {
        println!(
            "cargo:warning=codec libraries not found in {} (run migration/harness/zstream/fetch-codecs.ps1 or set CPP2RUST_CODEC_LIB)",
            dir.display()
        );
        return;
    }
    println!("cargo:rerun-if-changed={}", dir.display());
    println!("cargo:rustc-link-search=native={}", dir.display());
    let target_msvc = std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    let mut linked = false;
    for lib in LIBS {
        let file = if target_msvc { format!("{lib}.lib") } else { format!("lib{lib}.a") };
        if dir.join(&file).is_file() {
            println!("cargo:rustc-link-lib=static={lib}");
            linked = true;
        } else {
            println!("cargo:warning=codec library {file} not found in {}", dir.display());
        }
    }
    // The libraries are compiled /MT (static CRT) and the harness links the dynamic CRT: keep LIBCMT out so the C code
    // resolves against the CRT the harness uses.
    if linked && target_msvc {
        println!("cargo:rustc-link-arg-bins=/NODEFAULTLIB:libcmt.lib");
    }
}
