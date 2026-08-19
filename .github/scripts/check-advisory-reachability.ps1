$ErrorActionPreference = "Stop"

$advisoryId = "RUSTSEC-2026-0235"
$expectedRkyvVersion = "0.7.46"
$expectedChronoVersion = "0.4.45"
$expectedRustDecimalVersion = "1.42.1"
$repositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path

function Stop-Gate {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Message
    )

    Write-Output "::error file=.github/scripts/check-advisory-reachability.ps1::$Message"
    throw $Message
}

function Invoke-Cargo {
    param(
        [Parameter(Mandatory = $true)]
        [string[]]$Arguments
    )

    $output = @(& cargo @Arguments 2>&1)
    if ($LASTEXITCODE -ne 0) {
        Stop-Gate "cargo $($Arguments -join ' ') failed: $($output -join ' ')"
    }

    return $output
}

Push-Location $repositoryRoot
try {
    $auditConfig = Get-Content ".cargo/audit.toml" -Raw
    $configuredExceptions = @(
        [regex]::Matches($auditConfig, [regex]::Escape($advisoryId))
    )
    if ($configuredExceptions.Count -ne 1) {
        Stop-Gate ".cargo/audit.toml must contain exactly one $advisoryId exception."
    }

    $metadataJson = (Invoke-Cargo @(
        "metadata",
        "--locked",
        "--all-features",
        "--format-version",
        "1"
    )) -join "`n"
    $metadata = $metadataJson | ConvertFrom-Json

    $rkyvPackages = @(
        $metadata.packages | Where-Object { $_.name -eq "rkyv" }
    )
    if ($rkyvPackages.Count -ne 1 -or
        $rkyvPackages[0].version -ne $expectedRkyvVersion) {
        $versions = ($rkyvPackages | ForEach-Object { $_.version }) -join ", "
        Stop-Gate "The $advisoryId exception expects only rkyv $expectedRkyvVersion in Cargo.lock; found: $versions. Remove or re-review the exception."
    }

    $rustDecimalPackages = @(
        $metadata.packages | Where-Object {
            $_.name -eq "rust_decimal" -and
            $_.version -eq $expectedRustDecimalVersion
        }
    )
    if ($rustDecimalPackages.Count -ne 1) {
        Stop-Gate "The $advisoryId exception expects rust_decimal $expectedRustDecimalVersion. Remove or re-review the exception."
    }

    $rkyvOwners = @(
        $metadata.packages | Where-Object {
            @(
                $_.dependencies | Where-Object {
                    $_.name -eq "rkyv" -and
                    $null -eq $_.kind -and
                    $_.req -like "^0.7*"
                }
            ).Count -gt 0
        }
    )
    $owners = @(
        $rkyvOwners |
            ForEach-Object { "$($_.name) $($_.version)" } |
            Sort-Object
    )
    $expectedOwners = @(
        "chrono $expectedChronoVersion",
        "rust_decimal $expectedRustDecimalVersion"
    ) | Sort-Object
    $ownerDrift = @(Compare-Object $expectedOwners $owners)
    if ($ownerDrift.Count -gt 0) {
        Stop-Gate "The lock-only rkyv owners changed; found: $($owners -join ', '). Remove or re-review the $advisoryId exception."
    }

    $rkyvDependencies = @(
        $rkyvOwners | ForEach-Object {
            $_.dependencies | Where-Object {
                $_.name -eq "rkyv" -and
                $null -eq $_.kind -and
                $_.req -like "^0.7*"
            }
        }
    )
    if ($rkyvDependencies.Count -ne 2 -or
        @($rkyvDependencies | Where-Object { -not $_.optional }).Count -gt 0) {
        Stop-Gate "The rkyv 0.7 dependencies are no longer exactly two optional normal dependencies. Remove the $advisoryId exception."
    }

    $activePackages = Invoke-Cargo @(
        "tree",
        "--locked",
        "--all-features",
        "--target",
        "all",
        "--prefix",
        "none",
        "--format",
        "{p}"
    )
    $activeRkyv = @(
        $activePackages | Where-Object { $_ -match '^rkyv v' }
    )
    if ($activeRkyv.Count -gt 0) {
        Stop-Gate "$advisoryId is reachable in a supported all-feature/all-target build: $($activeRkyv -join ', ')"
    }

    $activeFeatures = Invoke-Cargo @(
        "tree",
        "--locked",
        "--all-features",
        "--target",
        "all",
        "--edges",
        "features",
        "--prefix",
        "none"
    )
    $activeRkyvFeatures = @(
        $activeFeatures | Where-Object {
            $_ -match '^chrono feature "rkyv(?:-(?:16|32|64|validation))?"' -or
            $_ -match '^rust_decimal feature "rkyv(?:-safe)?"'
        }
    )
    if ($activeRkyvFeatures.Count -gt 0) {
        Stop-Gate "$advisoryId is reachable through an enabled dependency feature: $($activeRkyvFeatures -join ', ')"
    }

    Write-Output "::notice::Verified $advisoryId is limited to inactive optional rkyv $expectedRkyvVersion lock metadata."
}
finally {
    Pop-Location
}
