#requires -version 5
<#
  Output-path benchmark: C++ baseline binary vs the Rust-output build (migration/bench/bench-output).
  Inputs are generated into a temp dir at run time and removed; nothing is committed.

  bench.ps1 -Baseline <ugrep-cpp.exe> -Candidate <ugrep-rust.exe> [-Runs 11] [-Lines 400000] [-Files 8] [-Limit 1.10]
  Output goes to NUL through cmd, so only the output code path and the search are timed. Reports medians per case
  (interleaved runs, one warm-up) and exits 1 when any ratio candidate/baseline exceeds -Limit.
#>
param(
  [Parameter(Mandatory)][string]$Baseline,
  [Parameter(Mandatory)][string]$Candidate,
  [int]$Runs = 11,
  [int]$Lines = 400000,
  [int]$Files = 8,
  [double]$Limit = 1.10
)
$ErrorActionPreference = 'Stop'
$Baseline = (Resolve-Path $Baseline).Path
$Candidate = (Resolve-Path $Candidate).Path
$dirName = 'ugrep-bench-' + [guid]::NewGuid().ToString('N')
$dir = Join-Path ([IO.Path]::GetTempPath()) $dirName
New-Item -ItemType Directory $dir | Out-Null

try {
  # deterministic generated corpus: many short lines, about 1 in 4 contains "needle"
  $rng = [Random]::new(12345)
  $words = 'alpha','bravo','charlie','delta','echo','foxtrot','golf','hotel','india','juliet','kilo','lima'
  for ($f = 0; $f -lt $Files; $f++) {
    $sb = [Text.StringBuilder]::new()
    for ($i = 0; $i -lt $Lines; $i++) {
      $w1 = $words[$rng.Next($words.Count)]; $w2 = $words[$rng.Next($words.Count)]
      if ($rng.Next(4) -eq 0) { [void]$sb.Append("$i $w1 needle $w2 $($rng.Next(100000))`n") }
      else { [void]$sb.Append("$i $w1 $w2 line $($rng.Next(100000))`n") }
    }
    [IO.File]::WriteAllText((Join-Path $dir "f$f.txt"), $sb.ToString())
  }

  $base = @(
    @{ name = 'plain';             args = "needle `"$dir`" -r" },
    @{ name = '-o';                args = "-o `"needle \w+`" `"$dir`" -r" },
    @{ name = '-c';                args = "-c needle `"$dir`" -r" },
    @{ name = '-n --color=always'; args = "-n --color=always needle `"$dir`" -r" },
    @{ name = '--json';            args = "--json needle `"$dir`" -r" }
  )
  # every case with one worker (-J1) and with the default thread count
  $cases = foreach ($t in @('-J1', 'default threads')) {
    foreach ($b in $base) {
      $pre = if ($t -eq '-J1') { '-J1 ' } else { '' }
      @{ name = "$($b.name) [$t]"; args = $pre + $b.args }
    }
  }

  function Time-One([string]$exe, [string]$a) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    cmd /c "`"$exe`" $a > NUL 2>&1"
    $sw.Stop()
    if ($LASTEXITCODE -ne 0) { throw "exit code ${LASTEXITCODE}: $exe $a" }
    $sw.Elapsed.TotalMilliseconds
  }
  # Output hash with a stable file order (--sort=name), so default threads are comparable
  function Hash-One([string]$exe, [string]$a) {
    $out = Join-Path ([IO.Path]::GetTempPath()) ($dirName + '-out.bin')
    cmd /c "`"$exe`" --sort=name $a > `"$out`" 2>&1"
    if ($LASTEXITCODE -ne 0) { throw "exit code ${LASTEXITCODE}: $exe $a" }
    (Get-FileHash -Algorithm SHA256 $out).Hash
  }
  function Median($v) { $s = $v | Sort-Object; $s[[int][math]::Floor($s.Count / 2)] }

  $failed = $false
  $rows = foreach ($c in $cases) {
    $hb = Hash-One $Baseline $c.args; $hr = Hash-One $Candidate $c.args
    if ($hb -ne $hr) { throw "output differs between baseline and candidate for case '$($c.name)'" }
    [void](Time-One $Baseline $c.args); [void](Time-One $Candidate $c.args)
    $b = @(); $r = @()
    for ($i = 0; $i -lt $Runs; $i++) {
      $b += Time-One $Baseline $c.args
      $r += Time-One $Candidate $c.args
    }
    $mb = Median $b; $mr = Median $r; $ratio = $mr / $mb
    if ($ratio -gt $Limit) { $failed = $true }
    [pscustomobject]@{ case = $c.name; cpp_ms = [math]::Round($mb, 1); rust_ms = [math]::Round($mr, 1); ratio = [math]::Round($ratio, 3); ok = ($ratio -le $Limit) }
  }
  "machine: $env:COMPUTERNAME, $([Environment]::ProcessorCount) logical CPUs; runs=$Runs lines=$Lines files=$Files"
  $rows | Format-Table -AutoSize | Out-String
  if ($failed) { "RESULT: FAIL (a ratio exceeds $Limit)"; exit 1 } else { "RESULT: PASS (all ratios <= $Limit)" }
}
finally {
  Remove-Item -Recurse -Force $dir, (Join-Path ([IO.Path]::GetTempPath()) ($dirName + '-out.bin')) -ErrorAction SilentlyContinue
}
