//! Rust side of the `src_leaves` component (src/glob.cpp, later zopen and zstream).
//! One bridge module (`ffi`); all boundary code is here. The bodies are stubs until the Port tasks.

// public so the signatures that are not called yet (stubs until the Port tasks) are part of the crate interface
pub mod signatures;
#[path = "ffi/zopen.rs"]
mod zopen_abi;
#[path = "ffi/zstream.rs"]
mod zstream_abi;

#[cxx::bridge(namespace = "ugrep_rust")]
mod ffi {
    extern "Rust" {
        /// Seam function `src_leaves.glob_match`. Arguments are the bytes of the C strings, without NUL.
        fn glob_match(
            pathname: &[u8],
            basename: &[u8],
            glob: &[u8],
            icase: bool,
            lead: bool,
            path: bool,
        ) -> bool;

        // Filename predicates of zstreambuf (unit zstream_core); the stream itself is the C ABI of ffi/zstream.rs.
        /// Seam function `src_leaves.zs_is_bz` (`zstreambuf::is_bz`).
        fn zs_is_bz(pathname: &[u8]) -> bool;
        /// Seam function `src_leaves.zs_is_xz` (`zstreambuf::is_xz`).
        fn zs_is_xz(pathname: &[u8]) -> bool;
        /// Seam function `src_leaves.zs_is_lz4` (`zstreambuf::is_lz4`).
        fn zs_is_lz4(pathname: &[u8]) -> bool;
        /// Seam function `src_leaves.zs_is_zstd` (`zstreambuf::is_zstd`).
        fn zs_is_zstd(pathname: &[u8]) -> bool;
        /// Seam function `src_leaves.zs_is_br` (`zstreambuf::is_br`).
        fn zs_is_br(pathname: &[u8]) -> bool;
        /// Seam function `src_leaves.zs_is_bz3` (`zstreambuf::is_bz3`).
        fn zs_is_bz3(pathname: &[u8]) -> bool;
        /// Seam function `src_leaves.zs_is_Z` (`zstreambuf::is_Z`).
        fn zs_is_z(pathname: &[u8]) -> bool;
        /// Seam function `src_leaves.zs_is_zip` (`zstreambuf::is_zip`).
        fn zs_is_zip(pathname: &[u8]) -> bool;
        /// Seam function `src_leaves.zs_is_7z` (`zstreambuf::is_7z`).
        fn zs_is_7z(pathname: &[u8]) -> bool;
        /// Seam function `src_leaves.zs_is_rar` (`zstreambuf::is_rar`).
        fn zs_is_rar(pathname: &[u8]) -> bool;
    }
}

fn glob_match(pathname: &[u8], basename: &[u8], glob: &[u8], icase: bool, lead: bool, path: bool) -> bool {
    signatures::glob_match(pathname, basename, glob, icase, lead, path)
}

fn zs_is_bz(pathname: &[u8]) -> bool {
    signatures::zstream_core::is_bz(pathname)
}

fn zs_is_xz(pathname: &[u8]) -> bool {
    signatures::zstream_core::is_xz(pathname)
}

fn zs_is_lz4(pathname: &[u8]) -> bool {
    signatures::zstream_core::is_lz4(pathname)
}

fn zs_is_zstd(pathname: &[u8]) -> bool {
    signatures::zstream_core::is_zstd(pathname)
}

fn zs_is_br(pathname: &[u8]) -> bool {
    signatures::zstream_core::is_br(pathname)
}

fn zs_is_bz3(pathname: &[u8]) -> bool {
    signatures::zstream_core::is_bz3(pathname)
}

fn zs_is_z(pathname: &[u8]) -> bool {
    signatures::zstream_core::is_z(pathname)
}

fn zs_is_zip(pathname: &[u8]) -> bool {
    signatures::zstream_core::is_zip(pathname)
}

fn zs_is_7z(pathname: &[u8]) -> bool {
    signatures::zstream_core::is_7z(pathname)
}

fn zs_is_rar(pathname: &[u8]) -> bool {
    signatures::zstream_core::is_rar(pathname)
}
