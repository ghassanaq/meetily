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

if (-not [Environment]::GetEnvironmentVariable("MEETING_ASSISTANT_LIVE_API_KEY", "Process")) {
    throw "Live Assist voice testing is not configured. MEETING_ASSISTANT_LIVE_API_KEY is missing."
}

Push-Location $repositoryRoot
try {
    & cargo test -p meetily reference_provider_preserves_voice_and_does_not_invent_commitment_history --lib -- --ignored --nocapture
    if ($LASTEXITCODE -ne 0) {
        throw "The Live Assist voice harness failed."
    }
} finally {
    Pop-Location
}

Write-Host "Harness records were appended to target\live-assist-voice-harness.jsonl."
