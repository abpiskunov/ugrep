# Builds the zopen seam driver with clang-cl source-based coverage: migration/out/src_leaves/coverage/cov_zopen.exe
$ErrorActionPreference = "Stop"
$root = Resolve-Path (Join-Path $PSScriptRoot "..\..\..")
$vw = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
$vs = & $vw -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath | Select-Object -First 1
if (-not $vs) { Write-Error "Visual Studio C++ tools not found"; exit 2 }
$candidates = @($env:CLANG_CL, (Join-Path $vs "VC\Tools\Llvm\x64\bin\clang-cl.exe")) +
  @(Get-ChildItem (Join-Path $env:ProgramFiles "Microsoft Visual Studio") -Recurse -Filter clang-cl.exe -ErrorAction SilentlyContinue | Where-Object { $_.FullName -like "*\x64\bin\*" } | ForEach-Object FullName)
$clang = $candidates | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
if (-not $clang) { Write-Error "clang-cl not found"; exit 127 }
$out = Join-Path $root "migration\out\src_leaves\coverage"
New-Item -ItemType Directory -Force $out | Out-Null
[IO.File]::WriteAllText((Join-Path $out "clang-dir.txt"), ((Split-Path $clang) -replace "\\","/"))
$flags = "/nologo /EHsc /std:c++17 /O0 /MT /DCPP2RUST_TAP /DZOPEN_FACADE /D_CRT_SECURE_NO_WARNINGS -fprofile-instr-generate -fcoverage-mapping"
$cmd = "call `"$vs\VC\Auxiliary\Build\vcvars64.bat`" >nul && cd /d `"$out`" && `"$clang`" $flags `"$root\migration\harness\src_leaves\zopen_driver.cpp`" `"$root\src\zopen_facade.cpp`" /Tc `"$root\src\zopen.c`" /Fe:cov_zopen.exe"
& $env:ComSpec /c $cmd
exit $LASTEXITCODE
