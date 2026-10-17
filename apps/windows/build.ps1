<#
.SYNOPSIS
Builds the Rust bridge, generates its C# bindings and builds the WinUI app.

.PARAMETER Target
Rust target triple. Defaults to the host: x86_64-pc-windows-msvc, or
aarch64-pc-windows-msvc on ARM64.

.PARAMETER Configuration
Debug or Release (default) for the .NET build.

.PARAMETER Publish
Publish a self-contained app to bin\publish\<rid> instead of only building.
#>
param(
    [ValidateSet('x86_64-pc-windows-msvc', 'aarch64-pc-windows-msvc')]
    [string]$Target = $(if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64') { 'aarch64-pc-windows-msvc' } else { 'x86_64-pc-windows-msvc' }),
    [ValidateSet('Debug', 'Release')]
    [string]$Configuration = 'Release',
    [switch]$Publish
)

$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
$root = (Resolve-Path (Join-Path $here '..\..')).Path
$arm = $Target -like 'aarch64-*'
$platform = if ($arm) { 'ARM64' } else { 'x64' }
$rid = if ($arm) { 'win-arm64' } else { 'win-x64' }

function Invoke-Step([string]$what, [scriptblock]$step) {
    Write-Host "==> $what"
    & $step
    if ($LASTEXITCODE -ne 0) { throw "$what failed ($LASTEXITCODE)" }
}

Push-Location $root
try {
    Invoke-Step "cargo build ($Target)" { cargo build --release -p photosink-bridge --target $Target }
    $dll = Join-Path $root "target\$Target\release\photosink_bridge.dll"
    $generated = Join-Path $here 'Generated'
    if (Test-Path $generated) { Remove-Item -Recurse -Force $generated }
    Invoke-Step 'uniffi-bindgen-cs' { uniffi-bindgen-cs --library $dll --out-dir $generated }
}
finally {
    Pop-Location
}

$project = Join-Path $here 'Photosink.csproj'
$common = @('-c', $Configuration, '-r', $rid, "-p:Platform=$platform", "-p:BridgeTriple=$Target")
if ($Publish) {
    $out = Join-Path $here "bin\publish\$rid"
    Invoke-Step 'dotnet publish' { dotnet publish $project @common --self-contained -o $out }
    Write-Host "Published to $out"
}
else {
    Invoke-Step 'dotnet build' { dotnet build $project @common }
}
