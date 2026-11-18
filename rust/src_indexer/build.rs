use std::env;

// The product link line decides which codec libraries the C++ was built with (`HAVE_LIBZ` and friends). The help
// text and option checks of the indexer follow it: UGREP_INDEXER_CODECS is a comma separated list of z, bz2, lzma,
// lz4, zstd, 7zip, brotli, bzip3; absent means a build without codecs.
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=CPP2RUST_CODEC_LIBS");
    println!("cargo:rerun-if-env-changed=PLATFORM");
    if env::var_os("CARGO_FEATURE_CODECS").is_some() {
        let listed = env::var("CPP2RUST_CODEC_LIBS").ok();
        // names of the autotools list (configure.ac) and the macro each stands for
        let names = [("zlib", "z"), ("bz2", "bz2"), ("lzma", "lzma"), ("lz4", "lz4"), ("zstd", "zstd"), ("viiz", "7zip")];
        let has = |n: &str| listed.as_ref().is_none_or(|l| l.split(',').any(|x| x == n));
        let mut codecs: Vec<&str> = names.iter().filter(|(n, _)| has(n)).map(|(_, m)| *m).collect();
        if let Some(l) = &listed {
            for extra in ["brotli", "bzip3"] {
                if l.split(',').any(|x| x == extra) {
                    codecs.push(extra);
                }
            }
        }
        // without zlib the C++ has no decompression at all
        if codecs.contains(&"z") {
            println!("cargo:rustc-env=UGREP_INDEXER_CODECS={}", codecs.join(","));
        }
    }
    if let Ok(platform) = env::var("PLATFORM") {
        println!("cargo:rustc-env=UGREP_PLATFORM={platform}");
    }
}
