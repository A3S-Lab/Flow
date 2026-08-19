[CmdletBinding()]
param(
    [Parameter()]
    [switch] $AllowDirty
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if (Test-Path variable:PSNativeCommandUseErrorActionPreference) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$repositoryRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot "../..") -ErrorAction Stop).Path
$cargo = (Get-Command cargo -ErrorAction Stop).Source

function Invoke-Cargo {
    param(
        [Parameter(Mandatory = $true)]
        [string[]] $Arguments
    )

    & $cargo @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "cargo $($Arguments -join ' ') failed with exit code $LASTEXITCODE"
    }
}

Push-Location $repositoryRoot
try {
    $packageArguments = @("package", "--locked")
    if ($AllowDirty) {
        $packageArguments += "--allow-dirty"
    }
    Invoke-Cargo -Arguments $packageArguments

    $metadataJson = & $cargo metadata --no-deps --format-version 1
    if ($LASTEXITCODE -ne 0) {
        throw "cargo metadata failed with exit code $LASTEXITCODE"
    }
    $metadata = ($metadataJson | Out-String | ConvertFrom-Json)
    $packages = @($metadata.packages | Where-Object { $_.name -eq "a3s-flow" })
    if ($packages.Count -ne 1) {
        throw "expected exactly one a3s-flow package, found $($packages.Count)"
    }

    if ([string]::IsNullOrWhiteSpace($env:CARGO_TARGET_DIR)) {
        $targetDirectory = Join-Path $repositoryRoot "target"
    }
    elseif ([IO.Path]::IsPathRooted($env:CARGO_TARGET_DIR)) {
        $targetDirectory = [IO.Path]::GetFullPath($env:CARGO_TARGET_DIR)
    }
    else {
        $targetDirectory = [IO.Path]::GetFullPath((Join-Path $repositoryRoot $env:CARGO_TARGET_DIR))
    }

    $packageManifest = Join-Path $targetDirectory "package/a3s-flow-$($packages[0].version)/Cargo.toml"
    if (-not (Test-Path -LiteralPath $packageManifest -PathType Leaf)) {
        throw "cargo package did not produce $packageManifest"
    }

    # The generated manifest contains the registry dependency graph that users
    # receive. Checking it independently catches APIs available only from a
    # development-time Git revision.
    Invoke-Cargo -Arguments @(
        "check",
        "--manifest-path", $packageManifest,
        "--all-targets",
        "--all-features"
    )
}
finally {
    Pop-Location
}
