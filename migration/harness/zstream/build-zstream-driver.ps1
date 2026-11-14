# Builds the zstream seam driver (tap on) against the codec-enabled reference configuration:
# migration/out/zstream/zstream_driver.exe. Run fetch-codecs.ps1 first. Optional -Asan builds with /fsanitize=address.
# Same macros as build-codec-ref.ps1: HAVE_LIBZ/BZ2/LZMA/LZ4/ZSTD, 7zip on, WITH_MAX_7ZIP_SIZE 1 GiB as in ugrep.cpp.
param([switch]$Asan)
$ErrorActionPreference = "Stop"
$root = Resolve-Path (Join-Path $PSScriptRoot "..\..\..")
$base = Split-Path $root
$lib = Join-Path $base "codec-build\lib"
$vw = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
$vs = & $vw -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath | Select-Object -First 1
if (-not $vs) { Write-Error "Visual Studio C++ tools not found"; exit 2 }
$out = Join-Path $root "migration\out\zstream"
New-Item -ItemType Directory -Force $out | Out-Null
$inc = "/I`"$base\zlib`" /I`"$base\bzip2`" /I`"$base\lz4\lib`" /I`"$base\zstd\lib`" /I`"$base\xz\src\liblzma\api`" /I`"$root\lzma\C`" /I`"$root\include`""
$crt = "/MT"
$extra = ""
if ($Asan) { $crt = "/MD"; $extra = "/fsanitize=address /Zi" }
$defs = "/DHAVE_LIBZ /DHAVE_LIBBZ2 /DHAVE_LIBLZMA /DHAVE_LIBLZ4 /DHAVE_LIBZSTD /DLZMA_API_STATIC /DWITH_MAX_7ZIP_SIZE=1073741824 /D_CRT_SECURE_NO_WARNINGS"
$z = "`"$root\src\zopen.c`""
$cmd = "call `"$vs\VC\Auxiliary\Build\vcvars64.bat`" >nul && cd /d `"$out`" && " +
  "cl /nologo /c /O2 $crt $extra /w /DZOPEN_FACADE /D_CRT_SECURE_NO_WARNINGS /std:c++17 /EHsc `"$root\src\zopen_facade.cpp`" && " +
  "cl /nologo /c /O2 $crt $extra /w /DZOPEN_FACADE /D_CRT_SECURE_NO_WARNINGS $z && " +
  "cl /nologo /O2 $crt $extra /w /EHsc /std:c++17 /DCPP2RUST_TAP $defs $inc `"$PSScriptRoot\zstream_driver.cpp`" zopen_facade.obj zopen.obj /Fe:zstream_driver.exe /link /LIBPATH:`"$lib`" zlib.lib bz2.lib lzma.lib lz4.lib zstd.lib viiz.lib"
& $env:ComSpec /c $cmd
exit $LASTEXITCODE
