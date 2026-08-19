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

$logPath = Join-Path ([System.IO.Path]::GetTempPath()) (
    "a3s-flow-msrv-{0}.log" -f [System.Guid]::NewGuid().ToString("N")
)

try {
    Write-Host "Checking with $(cargo "+$Toolchain" --version)"
    Write-Host "Compiler: $(rustc "+$Toolchain" --version)"

    & cargo "+$Toolchain" check --all-targets --all-features --locked 2>&1 |
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
        Write-GitHubErrorAnnotation -Title "MSRV check failed" -Message $summary

        if (
            $summary -match "lock file .+ needs to be updated" -and
            (Test-Path -LiteralPath "Cargo.lock")
        ) {
            $lockBackupPath = "$logPath.lock"
            Copy-Item -LiteralPath "Cargo.lock" -Destination $lockBackupPath -ErrorAction Stop

            try {
                Write-Host "::group::Resolve exact MSRV lockfile diagnostic"
                $resolveOutput = @(
                    & cargo "+$Toolchain" check --all-targets --all-features --offline 2>&1 |
                        ConvertFrom-NativeOutput
                )
                $resolveStatus = $LASTEXITCODE
                $resolveOutput | ForEach-Object { Write-Host $_ }

                $lockDiffLines = @(
                    & git diff --no-ext-diff --unified=1 -- "Cargo.lock" 2>&1 |
                        ConvertFrom-NativeOutput
                )
                $lockDiff = $lockDiffLines -join "`n"

                if ($resolveStatus -ne 0) {
                    $resolveTail = $resolveOutput | Select-Object -Last $AnnotationLineLimit
                    $resolutionFailure = @(
                        "Unlocked offline check failed with exit code $resolveStatus."
                        $resolveTail
                    ) -join "`n"
                    $lockDiff = @($lockDiff, $resolutionFailure) -join "`n`n"
                }
                Write-Host "::endgroup::"
            }
            finally {
                Copy-Item -LiteralPath $lockBackupPath -Destination "Cargo.lock" -Force -ErrorAction Stop
                Remove-Item -LiteralPath $lockBackupPath -Force -ErrorAction Stop
            }

            if (-not [string]::IsNullOrWhiteSpace($lockDiff)) {
                $chunkLength = 7000
                $chunkCount = [Math]::Ceiling($lockDiff.Length / $chunkLength)
                $emittedChunkCount = [Math]::Min($chunkCount, 8)

                for ($index = 0; $index -lt $emittedChunkCount; $index++) {
                    $offset = $index * $chunkLength
                    $length = [Math]::Min($chunkLength, $lockDiff.Length - $offset)
                    $chunk = $lockDiff.Substring($offset, $length)
                    $title = "MSRV lockfile diff {0}/{1}" -f ($index + 1), $chunkCount
                    Write-GitHubErrorAnnotation -Title $title -Message $chunk
                }
            }
        }
    }

    exit $status
}
finally {
    if (Test-Path -LiteralPath $logPath) {
        Remove-Item -LiteralPath $logPath -Force
    }
}
