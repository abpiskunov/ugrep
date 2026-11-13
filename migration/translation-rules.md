# Translation rules of this migration

Project conventions that every Port task follows. Start from the plugin rulebook
(`cpp-rust-idioms`); add a rule here only when a Wave review found the same question twice.

| # | C++ construct | Rust form | Since |
|---|---|---|---|
| 1 | (example) `Status` return + out-parameter | `Result<T, Error>`; the facade maps back | wave L0 |

A construct with no rule: mirror the C++ statement by statement, mark it
`// TODO(port): rule-gap <what>`, and list it in the unit record's `notes`.
