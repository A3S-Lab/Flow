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

$logPath = Join-Path ([System.IO.Path]::GetTempPath()) (
    "a3s-flow-msrv-{0}.log" -f [System.Guid]::NewGuid().ToString("N")
)

try {
    Write-Host "Checking with $(cargo "+$Toolchain" --version)"
    Write-Host "Compiler: $(rustc "+$Toolchain" --version)"

    & cargo "+$Toolchain" check --all-targets --all-features --locked 2>&1 |
        ForEach-Object {
            if ($_ -is [System.Management.Automation.ErrorRecord]) {
                $_.Exception.Message
            }
            else {
                $_.ToString()
            }
        } |
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

        $summary = $summary.Replace("%", "%25")
        $summary = $summary.Replace("`r", "%0D")
        $summary = $summary.Replace("`n", "%0A")
        Write-Output "::error title=MSRV check failed::$summary"
    }

    exit $status
}
finally {
    if (Test-Path -LiteralPath $logPath) {
        Remove-Item -LiteralPath $logPath -Force
    }
}
