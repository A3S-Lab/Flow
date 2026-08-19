[CmdletBinding()]
param(
    [Parameter()]
    [ValidateNotNullOrEmpty()]
    [string] $Toolchain = "1.88.0",

    [Parameter()]
    [ValidateRange(1, 100)]
    [int] $AnnotationLineLimit = 30
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Continue"

# Cargo writes ordinary progress messages to stderr. Windows PowerShell 5 wraps
# those lines in non-terminating error records, while PowerShell 7 can optionally
# turn native non-zero exit codes into terminating errors. Cargo's exit code is
# the portable source of truth here.
if (Test-Path variable:PSNativeCommandUseErrorActionPreference) {
    $PSNativeCommandUseErrorActionPreference = $false
}

function ConvertFrom-NativeOutput {
    process {
        if ($_ -is [System.Management.Automation.ErrorRecord]) {
            $_.Exception.Message
        }
        else {
            $_.ToString()
        }
    }
}

function Write-GitHubErrorAnnotation {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Title,

        [Parameter(Mandatory = $true)]
        [string] $Message
    )

    $escapedTitle = $Title.Replace("%", "%25")
    $escapedTitle = $escapedTitle.Replace("`r", "%0D")
    $escapedTitle = $escapedTitle.Replace("`n", "%0A")
    $escapedTitle = $escapedTitle.Replace(":", "%3A")
    $escapedTitle = $escapedTitle.Replace(",", "%2C")

    $escapedMessage = $Message.Replace("%", "%25")
    $escapedMessage = $escapedMessage.Replace("`r", "%0D")
    $escapedMessage = $escapedMessage.Replace("`n", "%0A")
    Write-Output "::error title=$escapedTitle::$escapedMessage"
}

$repositoryRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot "../..") -ErrorAction Stop).Path
$manifestPath = Join-Path $repositoryRoot "Cargo.toml"
$lockPath = Join-Path $repositoryRoot "Cargo.lock"
$logPath = Join-Path ([System.IO.Path]::GetTempPath()) (
    "a3s-flow-msrv-{0}.log" -f [System.Guid]::NewGuid().ToString("N")
)

try {
    Write-Host "Checking with $(cargo "+$Toolchain" --version)"
    Write-Host "Compiler: $(rustc "+$Toolchain" --version)"

    & cargo "+$Toolchain" check --manifest-path $manifestPath --all-targets --all-features --locked 2>&1 |
        ConvertFrom-NativeOutput |
        Tee-Object -FilePath $logPath
    $status = $LASTEXITCODE

    if ($status -eq 0) {
        exit 0
    }

    if ($env:GITHUB_ACTIONS -eq "true") {
        $lines = Get-Content -LiteralPath $logPath -Tail $AnnotationLineLimit -ErrorAction Stop
        $summary = $lines -join "`n"

        # Workflow-command annotations do not render ANSI control sequences.
        $ansiEscape = "{0}\[[0-?]*[ -/]*[@-~]" -f [char] 27
        $summary = $summary -replace $ansiEscape, ""
        if ($summary.Length -gt 8000) {
            $summary = $summary.Substring($summary.Length - 8000)
        }

        $lockBackupPath = "$logPath.lock"
        $lockBackedUp = $false
        try {
            Copy-Item -LiteralPath $lockPath -Destination $lockBackupPath -ErrorAction Stop
            $lockBackedUp = $true

            Write-Host "::group::Resolve exact MSRV lockfile diagnostic"
            $resolveOutput = @(
                & cargo "+$Toolchain" check --manifest-path $manifestPath --all-targets --all-features --offline 2>&1 |
                    ConvertFrom-NativeOutput
            )
            $resolveStatus = $LASTEXITCODE
            $resolveOutput | ForEach-Object { Write-Host $_ }

            $lockDiffLines = @(
                & git -C $repositoryRoot diff --no-ext-diff --unified=1 -- "Cargo.lock" 2>&1 |
                    ConvertFrom-NativeOutput
            )
            $lockDiagnostic = $lockDiffLines -join "`n"

            if ($resolveStatus -ne 0) {
                $resolveTail = $resolveOutput | Select-Object -Last $AnnotationLineLimit
                $resolutionFailure = @(
                    "Unlocked offline check failed with exit code $resolveStatus."
                    $resolveTail
                ) -join "`n"
                $lockDiagnostic = @($lockDiagnostic, $resolutionFailure) -join "`n`n"
            }
            elseif ([string]::IsNullOrWhiteSpace($lockDiagnostic)) {
                $lockDiagnostic = "Unlocked offline check succeeded but produced no Cargo.lock diff."
            }
            Write-Host "::endgroup::"
        }
        catch {
            $lockDiagnostic = "MSRV lockfile diagnostic failed: $($_.Exception.Message)"
        }
        finally {
            if ($lockBackedUp) {
                Copy-Item -LiteralPath $lockBackupPath -Destination $lockPath -Force -ErrorAction Stop
                Remove-Item -LiteralPath $lockBackupPath -Force -ErrorAction Stop
            }
        }

        $annotation = @($summary, $lockDiagnostic) -join "`n`n"
        $annotation = $annotation -replace $ansiEscape, ""
        if ($annotation.Length -gt 8000) {
            $annotation = $annotation.Substring(0, 8000)
        }
        Write-GitHubErrorAnnotation -Title "MSRV check failed" -Message $annotation
    }

    exit $status
}
finally {
    if (Test-Path -LiteralPath $logPath) {
        Remove-Item -LiteralPath $logPath -Force
    }
}
