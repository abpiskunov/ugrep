# Fetches the pinned codec sources (never vendored into the repo) into sibling folders of the repository root
# (<repo>\..\codec-src for the archives, <repo>\..\zlib, bzip2, xz, lz4, zstd for the sources, as msvc/ugrep.vcxproj
# expects) and builds static /MT x64 libraries into <repo>\..\codec-build\lib. Verifies SHA-256 against the pins below
# (the same values are recorded in migration/environment.json). Brotli and bzip3 are not fetched: not built.
$ErrorActionPreference = "Stop"
$root = Resolve-Path (Join-Path $PSScriptRoot "..\..\..")
$base = Split-Path $root
$arch = Join-Path $base "codec-src"
$build = Join-Path $base "codec-build"
$libdir = Join-Path $build "lib"
New-Item -ItemType Directory -Force $arch, $libdir | Out-Null
$pins = @(
  @{ name = "zlib";  dir = "zlib";  file = "zlib-1.3.1.tar.gz";  url = "https://github.com/madler/zlib/releases/download/v1.3.1/zlib-1.3.1.tar.gz";            sha = "9A93B2B7DFDAC77CEBA5A558A580E74667DD6FEDE4585B91EEFB60F03B72DF23" },
  @{ name = "bzip2"; dir = "bzip2"; file = "bzip2-1.0.8.tar.gz"; url = "https://sourceware.org/pub/bzip2/bzip2-1.0.8.tar.gz";                                      sha = "AB5A03176EE106D3F0FA90E381DA478DDAE405918153CCA248E682CD0C4A2269" },
  @{ name = "xz";    dir = "xz";    file = "xz-5.4.6.tar.gz";    url = "https://github.com/tukaani-project/xz/releases/download/v5.4.6/xz-5.4.6.tar.gz";      sha = "AEBA3E03BF8140DDEDF62A0A367158340520F6B384F75CA6045CCC6C0D43FD5C" },
  @{ name = "lz4";   dir = "lz4";   file = "lz4-1.10.0.tar.gz";   url = "https://github.com/lz4/lz4/releases/download/v1.10.0/lz4-1.10.0.tar.gz";             sha = "537512904744B35E232912055CCF8EC66D768639FF3ABE5788D90D792EC5F48B" },
  @{ name = "zstd";  dir = "zstd";  file = "zstd-1.5.6.tar.gz";   url = "https://github.com/facebook/zstd/releases/download/v1.5.6/zstd-1.5.6.tar.gz";          sha = "8C29E06CF42AACC1EAFC4077AE2EC6C6FCB96A626157E0593D5E82A34FD403C1" }
)
foreach ($p in $pins) {
  $f = Join-Path $arch $p.file
  if (-not (Test-Path $f)) { curl.exe -sSL --fail -o $f $p.url; if ($LASTEXITCODE) { Write-Error "download failed: $($p.url)"; exit 2 } }
  $h = (Get-FileHash $f -Algorithm SHA256).Hash
  if ($h -ne $p.sha) { Write-Error "SHA-256 mismatch for $($p.file): $h"; exit 2 }
  $dest = Join-Path $base $p.dir
  if (-not (Test-Path $dest)) {
    # the Windows bsdtar: a GNU tar earlier on PATH (Git Bash, CI's bash steps) reads "D:\..." as a remote host
    $tar = Join-Path $env:SystemRoot "System32\tar.exe"
    if (-not (Test-Path $tar)) { $tar = "tar" }
    & $tar -xzf $f -C $arch
    if ($LASTEXITCODE) { Write-Error "extract failed: $f"; exit 2 }
    Move-Item (Join-Path $arch ($p.file -replace "\.tar\.gz$", "")) $dest
  }
}
$vw = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
$vs = & $vw -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath | Select-Object -First 1
if (-not $vs) { Write-Error "Visual Studio C++ tools not found"; exit 2 }
function Invoke-Vc([string]$cmd) { & $env:ComSpec /c "call `"$vs\VC\Auxiliary\Build\vcvars64.bat`" >nul && $cmd"; if ($LASTEXITCODE) { Write-Error "failed: $cmd"; exit 1 } }
function Build-Lib([string]$name, [string]$srcdir, [string[]]$files, [string]$defs, [string]$inc) {
  if (Test-Path (Join-Path $libdir "$name.lib")) { return }
  $obj = Join-Path $build "obj\$name"
  New-Item -ItemType Directory -Force $obj | Out-Null
  $list = ($files | ForEach-Object { "`"$srcdir\$_`"" }) -join " "
  Invoke-Vc "cd /d `"$obj`" && cl /nologo /c /O2 /MT /w $defs $inc $list && lib /nologo /OUT:`"$libdir\$name.lib`" *.obj"
}
Build-Lib "zlib" "$base\zlib" @("adler32.c","compress.c","crc32.c","deflate.c","gzclose.c","gzlib.c","gzread.c","gzwrite.c","infback.c","inffast.c","inflate.c","inftrees.c","trees.c","uncompr.c","zutil.c") "/D_CRT_SECURE_NO_DEPRECATE /D_CRT_NONSTDC_NO_DEPRECATE" ""
Build-Lib "bz2" "$base\bzip2" @("blocksort.c","bzlib.c","compress.c","crctable.c","decompress.c","huffman.c","randtable.c") "/D_CRT_SECURE_NO_WARNINGS" ""
Build-Lib "lz4" "$base\lz4\lib" @("lz4.c","lz4hc.c","lz4frame.c","xxhash.c") "" ""
$zs = @()
foreach ($sub in "common","compress","decompress") { $zs += Get-ChildItem "$base\zstd\lib\$sub" -Filter *.c | ForEach-Object { "$sub\$($_.Name)" } }
Build-Lib "zstd" "$base\zstd\lib" $zs "/DZSTD_DISABLE_ASM /DXXH_NAMESPACE=ZSTD_" "/I`"$base\zstd\lib`" /I`"$base\zstd\lib\common`""
# 7zip support (not WITH_NO_7ZIP) is the in-repo lzma/C (libviiz); built with the flags of lzma/C/Makefile.am
Build-Lib "viiz" "$root\lzma\C" @("viizip.c","7zAlloc.c","7zArcIn.c","7zBuf.c","7zBuf2.c","7zCrc.c","7zCrcOpt.c","7zDec.c","7zFile.c","7zStream.c","Bcj2.c","Bra.c","Bra86.c","BraIA64.c","CpuArch.c","Delta.c","Lzma2Dec.c","LzmaDec.c","Ppmd7.c","Ppmd7Dec.c") "/DZ7_PPMD_SUPPORT /DZ7_EXTRACT_ONLY /DNDEBUG /D_REENTRANT /D_CRT_SECURE_NO_WARNINGS" ""
if (-not (Test-Path (Join-Path $libdir "lzma.lib"))) {
  $xb = Join-Path $build "xz"
  Invoke-Vc "cmake -S `"$base\xz`" -B `"$xb`" -G Ninja -DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=OFF -DCMAKE_POLICY_DEFAULT_CMP0091=NEW -DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded -DXZ_NLS=OFF -DXZ_TOOL_XZ=OFF -DXZ_TOOL_XZDEC=OFF -DXZ_TOOL_LZMADEC=OFF -DXZ_TOOL_LZMAINFO=OFF -DXZ_TOOL_SCRIPTS=OFF -DXZ_DOC=OFF >nul && cmake --build `"$xb`" --target liblzma"
  Copy-Item (Get-ChildItem $xb -Recurse -Filter "liblzma.lib" | Select-Object -First 1).FullName (Join-Path $libdir "lzma.lib")
}
Get-ChildItem $libdir | ForEach-Object { "$($_.Name) $($_.Length)" }
