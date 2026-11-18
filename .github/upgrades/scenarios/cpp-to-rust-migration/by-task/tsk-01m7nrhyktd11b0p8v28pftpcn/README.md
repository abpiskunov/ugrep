# S1: Rust-owned settings struct

- Design and member choices: `migration/units/settings.yaml`. Decision: `D-S1-supersede` in `migration/decisions.md`.
- `validation-plan.amendment.patch` (this folder) is the plan amendment the task intent asked for: `settings.obj` in src_core
  `retained_args`, a `settings-layout` check in the cleanup gate, `src/settings.cpp` in `cpp_sources`, comments recording the target
  end state (retained_args only RE-flex/third-party). It is not applied: `policy-scan` fails an implementation change that also edits
  the plan (`judge_and_judged`). Apply it in a `Plan amendment:` change (`git apply`).
- Benchmark (base 9f87c32 build vs S1 build, 63 MB input, 15 interleaved runs per case): median ratios 0.969, 1.023, 1.056, 1.005,
  1.011 (new/base); the plan has no performance threshold. Output in the sandbox `artifacts/bench.txt`.
- Autotools: automake/autoreconf/make are not on this worker, so `Makefile.in` was edited by hand (mirrors `stats.cpp`) and `./configure && make`
  was not run.
