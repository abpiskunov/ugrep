# Builds msvc/ugrep.sln. Usage: build-cpp.ps1 [-Configuration Base-Release] [-Extra '/p:CPP_RUST_IMPL_SRC_LEAVES=rust']
param(
  [string]$Configuration = "Base-Release",
  [string]$Platform = "x64",
  [string]$Extra = ""   # space-separated msbuild switches
)
$ExtraArgs = @($Extra -split '\s+' | Where-Object { $_ })
$ErrorActionPreference = "Stop"
$ms = Get-Command MSBuild.exe -ErrorAction SilentlyContinue
if ($ms) { $ms = $ms.Source } else {
  $vw = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
  $ms = & $vw -latest -products * -requires Microsoft.Component.MSBuild -find "MSBuild\**\Bin\MSBuild.exe" | Select-Object -First 1
}
if (-not $ms) { Write-Error "MSBuild.exe not found"; exit 2 }
$root = Resolve-Path (Join-Path $PSScriptRoot "..\..")
& $ms (Join-Path $root "msvc\ugrep.sln") /m /nologo "/p:Configuration=$Configuration" "/p:Platform=$Platform" /v:m @ExtraArgs
exit $LASTEXITCODE
