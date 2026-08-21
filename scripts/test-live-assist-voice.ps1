$ErrorActionPreference = "Stop"

$repositoryRoot = Split-Path -Parent $PSScriptRoot
$localEnvironmentPath = Join-Path $repositoryRoot ".env"
$localProviderPath = Join-Path $repositoryRoot ".env.provider"
$allowedLocalVariables = @(
    "MEETING_ASSISTANT_LIVE_API_KEY",
    "MEETING_ASSISTANT_LIVE_ENDPOINT",
    "MEETING_ASSISTANT_LIVE_MODEL"
)
foreach ($environmentPath in @($localEnvironmentPath, $localProviderPath)) {
    if (-not (Test-Path -LiteralPath $environmentPath)) { continue }
    foreach ($line in Get-Content -LiteralPath $environmentPath) {
        $separator = $line.IndexOf('=')
        if ($separator -le 0) { continue }
        $name = $line.Substring(0, $separator).Trim()
        if ($name -notin $allowedLocalVariables) { continue }
        if ([Environment]::GetEnvironmentVariable($name, "Process")) { continue }
        $value = $line.Substring($separator + 1).Trim().Trim('"').Trim("'")
        if ($value) { [Environment]::SetEnvironmentVariable($name, $value, "Process") }
    }
}

if (-not [Environment]::GetEnvironmentVariable("MEETING_ASSISTANT_LIVE_API_KEY", "Process")) {
    throw "Live Assist voice testing is not configured. MEETING_ASSISTANT_LIVE_API_KEY is missing."
}

Push-Location $repositoryRoot
try {
    & cargo test -p meetily reference_provider_ --lib -- --ignored --nocapture
    if ($LASTEXITCODE -ne 0) {
        throw "The Live Assist voice harness failed."
    }
} finally {
    Pop-Location
}

Write-Host "Harness records were appended to target\live-assist-voice-harness.jsonl."
