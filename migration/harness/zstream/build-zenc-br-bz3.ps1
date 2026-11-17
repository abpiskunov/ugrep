# Builds migration/out/zstream/zenc_br_bz3.exe (brotli and bzip3 encoder for the .br/.bz3 corpus generator) against the
# pinned codec libraries. Run fetch-codecs.ps1 first.
$ErrorActionPreference = "Stop"
$root = Resolve-Path (Join-Path $PSScriptRoot "..\..\..")
$base = Split-Path $root
$vw = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
$vs = & $vw -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath | Select-Object -First 1
if (-not $vs) { Write-Error "Visual Studio C++ tools not found"; exit 2 }
$out = Join-Path $root "migration\out\zstream"
New-Item -ItemType Directory -Force $out | Out-Null
$lib = Join-Path $base "codec-build\lib"
$cmd = "call `"$vs\VC\Auxiliary\Build\vcvars64.bat`" >nul && cd /d `"$out`" && cl /nologo /O2 /MT /w /I`"$base\brotli\c\include`" /I`"$base\bzip3\include`" `"$PSScriptRoot\zenc_br_bz3.c`" /Fe:zenc_br_bz3.exe /link /LIBPATH:`"$lib`" brotlienc.lib brotlicommon.lib bzip3.lib"
& $env:ComSpec /c $cmd
exit $LASTEXITCODE
