# Builds the codec-enabled C++ reference ugrep: HAVE_LIBZ, HAVE_LIBBZ2, HAVE_LIBLZMA, HAVE_LIBLZ4, HAVE_LIBZSTD
# (HAVE_LIBBROTLI and HAVE_LIBBZIP3 are not built; WITH_NO_7ZIP, WITH_ZIP_CRC32, WITH_MAX_7ZIP_SIZE are not defined).
# Output: msvc/bin/CodecRef-x64/ugrep.exe. Pure C++: every CPP_RUST_IMPL_* switch is cpp. Base-Release stays the product build.
# Run fetch-codecs.ps1 first.
param([string]$Extra = "")
$ErrorActionPreference = "Stop"
$root = Resolve-Path (Join-Path $PSScriptRoot "..\..\..")
$base = Split-Path $root
$libdir = Join-Path $base "codec-build\lib"
foreach ($l in "zlib","bz2","lz4","zstd","lzma","viiz") { if (-not (Test-Path (Join-Path $libdir "$l.lib"))) { Write-Error "missing $l.lib: run fetch-codecs.ps1"; exit 2 } }
$ms = Get-Command MSBuild.exe -ErrorAction SilentlyContinue
if ($ms) { $ms = $ms.Source } else {
  $vw = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
  $ms = & $vw -latest -products * -requires Microsoft.Component.MSBuild -find "MSBuild\**\Bin\MSBuild.exe" | Select-Object -First 1
}
$inc = "/I`"$base\zlib`" /I`"$base\bzip2`" /I`"$base\lz4\lib`" /I`"$base\zstd\lib`" /I`"$base\xz\src\liblzma\api`" /I`"$root\lzma\C`""
$env:CL = "/DHAVE_LIBZ /DHAVE_LIBBZ2 /DHAVE_LIBLZMA /DHAVE_LIBLZ4 /DHAVE_LIBZSTD $inc"
$env:_LINK_ = "/LIBPATH:`"$libdir`" zlib.lib bz2.lib lzma.lib lz4.lib zstd.lib viiz.lib"
# the project keeps its objects in msvc\bin\Base-Release-x64\ugrep\ and the CL macros are not tracked: clear them before and after so
# neither build reuses the other's objects
$stale = Join-Path $root "msvc\bin\Base-Release-x64\ugrep"
if (Test-Path $stale) { Remove-Item -Recurse -Force $stale }
$out = Join-Path $root "msvc\bin\CodecRef-x64\"
& $ms (Join-Path $root "msvc\ugrep.sln") /t:ugrep /m /nologo /v:m /p:Configuration=Base-Release /p:Platform=x64 `
  "/p:OutDir=$out" /p:CPP_RUST_IMPL_SRC_LEAVES=cpp /p:CPP_RUST_IMPL_SRC_LEAVES_ZSTREAM=cpp $Extra
$rc = $LASTEXITCODE
if (Test-Path $stale) { Remove-Item -Recurse -Force $stale }
exit $rc
