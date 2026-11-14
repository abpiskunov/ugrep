use std::path::PathBuf;

// The `codecs` feature (default) makes the Rust zstream call the retained C codec libraries (zlib, bzip2, xz, lz4, zstd,
// viiz) through `extern "C"`. Without it no codec symbol is referenced (cfg `ugrep_no_codecs`), the same as the C++ in a
// configuration that defines no HAVE_LIB*.
//
// Where the libraries come from:
// - the product build (msvc/rust.targets) sets CPP2RUST_CODECS_FROM_LINKER: the final link line of the product, with its
//   project references, provides them, and nothing is searched or bundled here;
// - tests and the replay harness use the static libraries built outside the repository by
//   migration/harness/zstream/fetch-codecs.ps1 (/MT, x64) in `<repo>\..\codec-build\lib` (CPP2RUST_CODEC_LIB names another
//   directory). They are linked as non-bundled static libraries, so they never end up inside the umbrella staticlib.
// A missing library is a build error.
const LIBS: [&str; 6] = ["zlib", "bz2", "lzma", "lz4", "zstd", "viiz"];

fn main() {
    println!("cargo:rerun-if-changed=src/lib.rs");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=CPP2RUST_CODEC_LIB");
    println!("cargo:rerun-if-env-changed=CPP2RUST_CODECS_FROM_LINKER");
    println!("cargo:rustc-check-cfg=cfg(ugrep_no_codecs)");
    let target_msvc = std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    cxx_build::bridge("src/lib.rs").std("c++14").compile("src_leaves_bridge");

    if std::env::var_os("CARGO_FEATURE_CODECS").is_none() {
        println!("cargo:rustc-cfg=ugrep_no_codecs");
        return;
    }
    if std::env::var_os("CPP2RUST_CODECS_FROM_LINKER").is_some() {
        return;
    }
    let dir = match std::env::var_os("CPP2RUST_CODEC_LIB") {
        Some(d) => PathBuf::from(d),
        None => {
            let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
            // <repo>/rust/src_leaves -> <repo>/../codec-build/lib
            manifest.join("..").join("..").join("..").join("codec-build").join("lib")
        }
    };
    let file_of = |lib: &str| if target_msvc { format!("{lib}.lib") } else { format!("lib{lib}.a") };
    for lib in LIBS {
        if !dir.join(file_of(lib)).is_file() {
            panic!(
                "codec library {} not found in {}: run migration/harness/zstream/fetch-codecs.ps1, set CPP2RUST_CODEC_LIB, \
                 or build without the `codecs` feature",
                file_of(lib),
                dir.display()
            );
        }
    }
    println!("cargo:rerun-if-changed={}", dir.display());
    println!("cargo:rustc-link-search=native={}", dir.display());
    for lib in LIBS {
        println!("cargo:rustc-link-lib=static:-bundle={lib}");
    }
    // The libraries are compiled /MT and the Rust test executables use the dynamic CRT.
    if target_msvc {
        println!("cargo:rustc-link-arg=/NODEFAULTLIB:libcmt.lib");
    }
}