# help.txt

`help.txt` is the standard output of `ugrep --help` recorded from the C++ build (`msvc/bin/Base-Release-x64/ugrep.exe`,
Behavior round 16, cases `hm-help*`, behaviors B-src_core-128 and following). It is the text the Rust port embeds with
`include_str!`; it is never retyped.

- Line endings: the Windows C++ writes the text through a text-mode `std::cout`, so the recorded trace and a raw capture
  have CRLF. The file is stored with LF and `.gitattributes` (`help.txt -text`) keeps it LF in every checkout, whatever
  `core.autocrlf` or `text=auto` say (CI checks out on windows-latest). `raw.replace("\r\n", "\n") == help.txt` for a
  fresh run, which a reviewer can check with the C++ binary. On Windows the port converts LF to CRLF itself when it
  writes the text, so its bytes equal the C++ output.
- The text ends with the trailer printed by `help(const char *what)` when WHAT is empty:
  `\n    Specify `ugrep --help WHAT' for help on WHAT.\n\n`.
- Exit code of `--help` is 2 (B-src_core-128, suspicious, for Triage).
