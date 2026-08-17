param(
    [string]$BinaryPath
)

$ErrorActionPreference = "Stop"

$repositoryRoot = Split-Path -Parent $PSScriptRoot
$localEnvironmentPath = Join-Path $repositoryRoot ".env"
if (-not [Environment]::GetEnvironmentVariable("MEETING_ASSISTANT_LIVE_API_KEY", "Process") -and (Test-Path -LiteralPath $localEnvironmentPath)) {
    foreach ($line in Get-Content -LiteralPath $localEnvironmentPath) {
        $separator = $line.IndexOf('=')
        if ($separator -le 0) { continue }
        $name = $line.Substring(0, $separator).Trim()
        if ($name -ne "MEETING_ASSISTANT_LIVE_API_KEY") { continue }
        $value = $line.Substring($separator + 1).Trim().Trim('"').Trim("'")
        if ($value) { [Environment]::SetEnvironmentVariable($name, $value, "Process") }
    }
}

$requiredVariables = @("MEETING_ASSISTANT_LIVE_API_KEY")
$missing = $requiredVariables | Where-Object {
    -not [Environment]::GetEnvironmentVariable($_, "Process")
}
if ($missing.Count -gt 0) {
    throw "Live Assist is not configured. Missing environment variable(s): $($missing -join ', ')."
}

if (-not $BinaryPath) {
    $candidates = @(
        (Join-Path $repositoryRoot "target\release\meetily.exe"),
        (Join-Path $repositoryRoot "frontend\src-tauri\target\release\meetily.exe")
    )
    $BinaryPath = $candidates | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
}

if (-not $BinaryPath -or -not (Test-Path -LiteralPath $BinaryPath)) {
    throw "The release app was not found. Build it once with 'corepack pnpm --dir frontend tauri:build:cpu', then use this launcher before meetings."
}

$resolvedBinary = (Resolve-Path -LiteralPath $BinaryPath).Path
Start-Process -FilePath $resolvedBinary -ArgumentList "--live-assist" -WorkingDirectory (Split-Path -Parent $resolvedBinary)
