use std::path::PathBuf;

// The retained C codec libraries (zlib, bzip2, xz, lz4, zstd, viiz) are called through `extern "C"` by the Rust zstream
// codecs. They are built outside the repository by migration/harness/zstream/fetch-codecs.ps1 (static, /MT, x64) into
// `<repo>\..\codec-build\lib`; CPP2RUST_CODEC_LIB names another directory. As static libraries they are bundled into
// the Rust library, so the umbrella lib and the unit tests resolve them.
const LIBS: [&str; 6] = ["zlib", "bz2", "lzma", "lz4", "zstd", "viiz"];

fn main() {
    cxx_build::bridge("src/lib.rs").std("c++14").compile("src_leaves_bridge");
    println!("cargo:rerun-if-changed=src/lib.rs");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=CPP2RUST_CODEC_LIB");

    let dir = match std::env::var_os("CPP2RUST_CODEC_LIB") {
        Some(d) => PathBuf::from(d),
        None => {
            let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
            // <repo>/rust/src_leaves -> <repo>/../codec-build/lib
            manifest.join("..").join("..").join("..").join("codec-build").join("lib")
        }
    };
    if !dir.is_dir() {
        println!("cargo:warning=codec libraries not found in {} (run migration/harness/zstream/fetch-codecs.ps1)", dir.display());
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
    // The libraries are compiled /MT and the Rust test executables use the dynamic CRT.
    if linked && target_msvc {
        println!("cargo:rustc-link-arg=/NODEFAULTLIB:libcmt.lib");
    }
}
