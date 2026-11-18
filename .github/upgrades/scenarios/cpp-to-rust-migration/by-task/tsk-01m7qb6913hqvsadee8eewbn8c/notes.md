# Switch over: output (Rust-owned output buffer)

Design: Rust owns the buffer blocks, flush (--null-data, --width), Tree state and Dump::hex
(`rust/src_core/src/output_buf.rs`, C ABI in `ffi/output_buf.rs`). C++ `chr/str/num/nl` write through a
cursor (`cur`/`end`) into the Rust block; Rust is called only on block-full, flush and reset.
`src/output.cpp` is deleted (removed from msvc project and Makefile.am); the facade is inline in `output_rust.hpp`.

Gate: `gate-check --gate switch --target src_core` PASS (migration/out/gates/switch/src_core/report.json),
surface 5034 cases, 0 divergent. cargo test (74) and clippy -D warnings pass for src_core.

Benchmark (400k lines, best of 7, output to file; C++ = commit 82ab1e5, pre = Rust format + C++ buffer):

| mode | cpp ms | pre ms | new ms | new/cpp |
|---|---|---|---|---|
| plain | 144.4 | 434.2 | 159.0 | 1.10 |
| -o | 132.5 | 484.9 | 185.7 | 1.40 |
| -c | 95.0 | 436.2 | 100.7 | 1.06 |
| -n --color=always | 274.9 | 1005.7 | 755.0 | 2.75 |
| --json | 596.5 | 1943.8 | 1349.8 | 2.26 |

NOT MET: -o, -n --color=always and --json exceed 1.1x. The remaining cost is the per-line header/format
bridge call (Vec<u8> result + match snapshot), ported earlier and owned by the S4 perf task
tsk_01M7R8WD4HJADDDA21H2AHVXJX; not duplicated here. Output bytes are identical across all three builds.

retained_args (validation-plan.yaml line 63 still requires output[.]obj) is protected: operator plan PR needed.
Autotools is proven by PR CI.
