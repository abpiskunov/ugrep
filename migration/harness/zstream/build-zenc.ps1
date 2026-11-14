# Builds migration/out/zstream/zenc.exe (lz4 and zstd frame encoder for the corpus generator) against the pinned codec libraries.
$ErrorActionPreference = "Stop"
$root = Resolve-Path (Join-Path $PSScriptRoot "..\..\..")
$base = Split-Path $root
$vw = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
$vs = & $vw -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath | Select-Object -First 1
if (-not $vs) { Write-Error "Visual Studio C++ tools not found"; exit 2 }
$out = Join-Path $root "migration\out\zstream"
New-Item -ItemType Directory -Force $out | Out-Null
$lib = Join-Path $base "codec-build\lib"
$cmd = "call `"$vs\VC\Auxiliary\Build\vcvars64.bat`" >nul && cd /d `"$out`" && cl /nologo /O2 /MT /w /I`"$base\lz4\lib`" /I`"$base\zstd\lib`" `"$PSScriptRoot\zenc.c`" /Fe:zenc.exe /link /LIBPATH:`"$lib`" lz4.lib zstd.lib"
& $env:ComSpec /c $cmd
exit $LASTEXITCODE
