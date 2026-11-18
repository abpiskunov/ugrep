# help.txt

`help.txt` is the standard output of `ugrep --help` recorded from the C++ build (`msvc/bin/Base-Release-x64/ugrep.exe`,
Behavior round 16, cases `hm-help*`, behaviors B-src_core-127 and following). It is the text the Rust port embeds with
`include_str!`; it is never retyped.

- Line endings: the Windows C++ writes the text through a text-mode `std::cout`, so the recorded trace and a raw capture
  have CRLF. The file is stored with LF (the repository's `* text=auto`); `raw.replace("\r\n", "\n") == help.txt` for a
  fresh run, which a reviewer can check with the C++ binary. A Rust build that writes with the same stdout mode produces the
  same bytes as the C++.
- The text ends with the trailer printed by `help(const char *what)` when WHAT is empty:
  `\n    Specify `ugrep --help WHAT' for help on WHAT.\n\n`.
- Exit code of `--help` is 2 (B-src_core-127, suspicious, for Triage).
