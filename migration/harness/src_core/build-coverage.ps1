# Builds ugrep with clang-cl source-based coverage of the src_core sources into
# migration/out/src_core/coverage/ugrep-cov.exe. The plain C++ product (CPP_RUST_IMPL_SRC_CORE=cpp) is built by
# MSBuild first (build-cpp.ps1); then src/ugrep.cpp, output.cpp, stats.cpp and cnf.cpp are recompiled with clang-cl
# (-fprofile-instr-generate -fcoverage-mapping, same defines and includes as MSBuild used) and linked in place of the
# MSVC objects. The ClangCL MSBuild toolset is not needed, only clang-cl and the llvm tools.
$ErrorActionPreference = "Stop"
$root = Resolve-Path (Join-Path $PSScriptRoot "..\..\..")
$vw = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
$vs = & $vw -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath | Select-Object -First 1
if (-not $vs) { Write-Error "Visual Studio C++ tools not found"; exit 2 }
$clang = @($env:CLANG_CL) + @(Get-ChildItem (Join-Path $env:ProgramFiles "Microsoft Visual Studio") -Recurse -Filter clang-cl.exe -ErrorAction SilentlyContinue |
  Where-Object { $_.FullName -like "*\x64\bin\*" } | ForEach-Object FullName) | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
if (-not $clang) { Write-Error "clang-cl not found (set CLANG_CL)"; exit 127 }
$llvm = Split-Path $clang
$rt = Get-ChildItem (Join-Path (Split-Path $llvm) "lib") -Recurse -Filter "clang_rt.profile-x86_64.lib" | Select-Object -First 1
if (-not $rt) { Write-Error "clang_rt.profile-x86_64.lib not found"; exit 127 }

# 1. the plain C++ product, for the objects and libraries
& (Join-Path $root "migration\harness\build-cpp.ps1") -Extra "/p:CPP_RUST_IMPL_SRC_CORE=cpp"
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
$bin = Join-Path $root "msvc\bin\Base-Release-x64"
$cl = Get-Content (Join-Path $bin "ugrep\ugrep.tlog\CL.command.1.tlog") -Encoding Unicode
$defs = [regex]::Matches($cl[1], '/D (\S+)') | ForEach-Object { "/D" + $_.Groups[1].Value }

$out = Join-Path $root "migration\out\src_core\coverage"
New-Item -ItemType Directory -Force $out | Out-Null
[IO.File]::WriteAllText((Join-Path $out "llvm-dir.txt"), ($llvm -replace "\\", "/"))
$flags = @("/c", "/nologo", "/std:c++17", "/O0", "/EHsc", "/MT", "/Zc:inline", "/permissive-", "/w", "/TP", "-fprofile-instr-generate", "-fcoverage-mapping",
  "/I`"$root\build\cargo\ugrep\cxxbridge`"", "/I../include", "/I./include_ugrep", "/FI`"$root\msvc\include_ugrep\config.h`"") + $defs
$srcs = "ugrep", "output", "stats", "cnf"
$objs = @()
foreach ($s in $srcs) {
  $o = Join-Path $out "$s.cov.obj"
  $objs += $o
  $cmd = "call `"$vs\VC\Auxiliary\Build\vcvars64.bat`" >nul && cd /d `"$root\msvc`" && `"$clang`" $($flags -join ' ') `"$root\src\$s.cpp`" /Fo`"$o`""
  & $env:ComSpec /c $cmd
  if ($LASTEXITCODE -ne 0) { Write-Error "clang-cl failed on $s.cpp"; exit $LASTEXITCODE }
}
# 2. link: the MSVC objects except the four, plus the instrumented ones, the same libraries
$skip = $srcs | ForEach-Object { "$_.obj" }
$msvcObjs = Get-ChildItem (Join-Path $bin "ugrep") -Filter *.obj | Where-Object { $skip -notcontains $_.Name.ToLower() } | ForEach-Object { "`"$($_.FullName)`"" }
$libs = Get-ChildItem (Join-Path $root "msvc\bin\None-x64") -Filter *.lib | ForEach-Object { "`"$($_.FullName)`"" }
$umbrella = Join-Path $root "build\cargo\ugrep\release\rust_umbrella.lib"
$sys = "ntdll.lib userenv.lib bcrypt.lib ws2_32.lib advapi32.lib kernel32.lib user32.lib gdi32.lib winspool.lib comdlg32.lib shell32.lib ole32.lib oleaut32.lib uuid.lib"
$rsp = Join-Path $out "link.rsp"
$lnk = @("/OUT:`"$out\ugrep-cov.exe`"", "/INCREMENTAL:NO", "/NOLOGO", "/SUBSYSTEM:CONSOLE", "/MACHINE:X64", "/DEBUG", "/OPT:REF", "/LTCG", "`"$umbrella`"", $sys) + $msvcObjs + ($objs | ForEach-Object { "`"$_`"" }) + $libs + "`"$($rt.FullName)`""
[IO.File]::WriteAllLines($rsp, $lnk)
& $env:ComSpec /c "call `"$vs\VC\Auxiliary\Build\vcvars64.bat`" >nul && link @`"$rsp`""
exit $LASTEXITCODE
