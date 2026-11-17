# Builds ugrep-indexer with clang-cl source-based coverage of src/ugrep-indexer.cpp into
# migration/out/src_indexer/coverage/ugrep-indexer-cov.exe. The plain C++ product (CPP_RUST_IMPL_SRC_INDEXER=cpp) is
# built by MSBuild first (build-cpp.ps1); then src/ugrep-indexer.cpp is recompiled with clang-cl
# (-fprofile-instr-generate -fcoverage-mapping, same defines and includes as MSBuild used) and linked in place of the
# MSVC object. The ClangCL MSBuild toolset is not needed, only clang-cl and the llvm tools.
$ErrorActionPreference = "Stop"
$root = Resolve-Path (Join-Path $PSScriptRoot "..\..\..")
$vw = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
$vs = & $vw -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath | Select-Object -First 1
if (-not $vs) { Write-Error "Visual Studio C++ tools not found"; exit 2 }
$clang = @($env:CLANG_CL) + @(Join-Path $vs "VC\Tools\Llvm\x64\bin\clang-cl.exe") + @(Get-ChildItem (Join-Path $env:ProgramFiles "Microsoft Visual Studio") -Recurse -Filter clang-cl.exe -ErrorAction SilentlyContinue |
  Where-Object { $_.FullName -like "*\x64\bin\*" } | ForEach-Object FullName) | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
if (-not $clang) { Write-Error "clang-cl not found (set CLANG_CL)"; exit 127 }
$llvm = Split-Path $clang
$rt = Get-ChildItem (Join-Path (Split-Path $llvm) "lib") -Recurse -Filter "clang_rt.profile-x86_64.lib" | Select-Object -First 1
if (-not $rt) { Write-Error "clang_rt.profile-x86_64.lib not found"; exit 127 }

# 1. the plain C++ product, for the objects and libraries
& (Join-Path $root "migration\harness\build-cpp.ps1") -Extra "/p:CPP_RUST_IMPL_SRC_INDEXER=cpp"
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
$bin = Join-Path $root "msvc\bin\Base-Release-x64"
$cl = Get-Content (Join-Path $bin "ugrep-indexer\ugrep-indexer.tlog\CL.command.1.tlog") -Encoding Unicode
$defs = [regex]::Matches($cl[1], '/D (\S+)') | ForEach-Object { "/D" + $_.Groups[1].Value }

$out = Join-Path $root "migration\out\src_indexer\coverage"
New-Item -ItemType Directory -Force $out | Out-Null
[IO.File]::WriteAllText((Join-Path $out "llvm-dir.txt"), ($llvm -replace "\\", "/"))
$flags = @("/c", "/nologo", "/std:c++17", "/O0", "/EHsc", "/MT", "/Zc:inline", "/permissive-", "/w", "/TP", "-fprofile-instr-generate", "-fcoverage-mapping",
  "/I`"$root\build\cargo\ugrep-indexer\cxxbridge`"", "/I../include", "/I./include_ugrep", "/FI`"$root\msvc\include_ugrep\config.h`"") + $defs
$o = Join-Path $out "ugrep-indexer.cov.obj"
$cmd = "call `"$vs\VC\Auxiliary\Build\vcvars64.bat`" >nul && cd /d `"$root\msvc`" && `"$clang`" $($flags -join ' ') `"$root\src\ugrep-indexer.cpp`" /Fo`"$o`""
& $env:ComSpec /c $cmd
if ($LASTEXITCODE -ne 0) { Write-Error "clang-cl failed on ugrep-indexer.cpp"; exit $LASTEXITCODE }

# 2. link: the MSVC objects except ugrep-indexer.obj, plus the instrumented one, the same libraries
$msvcObjs = Get-ChildItem (Join-Path $bin "ugrep-indexer") -Filter *.obj | Where-Object { $_.Name.ToLower() -ne "ugrep-indexer.obj" } | ForEach-Object { "`"$($_.FullName)`"" }
$umbrella = Join-Path $root "build\cargo\ugrep-indexer\release\rust_umbrella.lib"
$sys = "ntdll.lib userenv.lib bcrypt.lib ws2_32.lib advapi32.lib kernel32.lib user32.lib gdi32.lib winspool.lib comdlg32.lib shell32.lib ole32.lib oleaut32.lib uuid.lib"
$rsp = Join-Path $out "link.rsp"
$lnk = @("/OUT:`"$out\ugrep-indexer-cov.exe`"", "/INCREMENTAL:NO", "/NOLOGO", "/SUBSYSTEM:CONSOLE", "/MACHINE:X64", "/DEBUG", "/OPT:REF", "/LTCG", "`"$umbrella`"", $sys) + $msvcObjs + "`"$o`"" + "`"$($rt.FullName)`""
[IO.File]::WriteAllLines($rsp, $lnk)
& $env:ComSpec /c "call `"$vs\VC\Auxiliary\Build\vcvars64.bat`" >nul && link @`"$rsp`""
exit $LASTEXITCODE
