# Builds the seam driver (tap on) from the C++ glob.cpp: migration/out/src_leaves/glob_driver.exe
# Optional -Asan builds with /fsanitize=address.
param([switch]$Asan)
$ErrorActionPreference = "Stop"
$root = Resolve-Path (Join-Path $PSScriptRoot "..\..\..")
$vw = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
$vs = & $vw -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath | Select-Object -First 1
if (-not $vs) { Write-Error "Visual Studio C++ tools not found"; exit 2 }
$out = Join-Path $root "migration\out\src_leaves"
New-Item -ItemType Directory -Force $out | Out-Null
$flags = "/nologo /EHsc /std:c++17 /O2 /MT /DCPP2RUST_TAP /D_CRT_SECURE_NO_WARNINGS"
if ($Asan) { $flags = $flags.Replace("/MT", "/MD") + " /fsanitize=address /Zi" }
$cmd = "call `"$vs\VC\Auxiliary\Build\vcvars64.bat`" >nul && cd /d `"$out`" && cl $flags `"$root\migration\harness\src_leaves\glob_driver.cpp`" `"$root\src\glob.cpp`" /Fe:glob_driver.exe"
& $env:ComSpec /c $cmd
exit $LASTEXITCODE
