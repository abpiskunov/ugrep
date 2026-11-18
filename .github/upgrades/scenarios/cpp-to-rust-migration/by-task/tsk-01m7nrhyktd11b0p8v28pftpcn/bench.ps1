# Hot-loop benchmark: base-commit ugrep vs S1 ugrep on identical generated input.
# Usage: bench.ps1 -Base <exe> -New <exe> [-Runs 15] [-Out bench-output.txt]
param([string]$Base, [string]$New, [int]$Runs = 15, [string]$Out = "bench-output.txt")
$ErrorActionPreference = "Stop"
$dir = Join-Path ([IO.Path]::GetTempPath()) "ugrep-bench"
New-Item -ItemType Directory -Force $dir | Out-Null
$f = Join-Path $dir "in.txt"
$rng = [Random]::new(42)
$words = 1..2000 | ForEach-Object { -join (1..($rng.Next(3,10)) | ForEach-Object { [char]$rng.Next(97,123) }) }
$sb = [Text.StringBuilder]::new()
for ($i = 0; $i -lt 400000; $i++) {
  [void]$sb.AppendLine((1..10 | ForEach-Object { $words[$rng.Next(0,$words.Count)] }) -join " ")
}
[IO.File]::WriteAllText($f, $sb.ToString())
$cases = @(
  @{n="literal";  a=@("-c","needle_not_here",$f)},
  @{n="word";     a=@("-c","-w",$words[5],$f)},
  @{n="regex";    a=@("-c","-E","($($words[1])|$($words[2]))[a-z]+ [a-z]{3}",$f)},
  @{n="icase";    a=@("-c","-i",$words[7],$f)},
  @{n="output";   a=@("-n","--color=never",$words[9],$f)}
)
$lines = @("runs=$Runs input_bytes=$((Get-Item $f).Length)")
foreach ($c in $cases) {
  $t = @{base=@(); new=@()}
  foreach ($w in "base","new") { & (Get-Variable -ValueOnly ([string]$w -replace "^b","B" -replace "^n","N")) @($c.a) | Out-Null }
  for ($i = 0; $i -lt $Runs; $i++) {
    foreach ($w in "base","new") {
      $exe = if ($w -eq "base") { $Base } else { $New }
      $t[$w] += (Measure-Command { & $exe @($c.a) | Out-Null }).TotalMilliseconds
    }
  }
  $mb = ($t.base | Sort-Object)[[int]($Runs/2)]; $mn = ($t.new | Sort-Object)[[int]($Runs/2)]
  $lines += "{0,-8} base_median_ms={1:N1} new_median_ms={2:N1} ratio={3:N3}" -f $c.n,$mb,$mn,($mn/$mb)
}
$lines | Tee-Object -FilePath $Out
