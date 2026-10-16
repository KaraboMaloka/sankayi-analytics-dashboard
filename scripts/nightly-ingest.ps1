# Sankayi nightly ingest — runs all three data pulls in sequence and logs output.
# Registered as the Windows scheduled task "SankayiNightlyIngest".
#
# Order: Instagram (audience + posts + stories) -> Meta Ads (spend) -> Quicket
# (ticket sales). Each is idempotent, so a re-run just refreshes in place.
#
# The binaries read backend\.env for DATABASE_URL and the Meta/Quicket tokens,
# so we MUST run from the backend directory (dotenvy loads ./.env relative to CWD).

$ErrorActionPreference = 'Continue'
$root = 'C:\gitrepos\sankayi-analytics-dashboard'
Set-Location "$root\backend"

$logDir = "$root\logs"
if (-not (Test-Path $logDir)) { New-Item -ItemType Directory -Force -Path $logDir | Out-Null }
$log = Join-Path $logDir ("ingest-{0}.log" -f (Get-Date -Format 'yyyy-MM-dd'))

function Log($msg) { "{0}  {1}" -f (Get-Date -Format 'yyyy-MM-dd HH:mm:ss'), $msg | Out-File -Append -Encoding utf8 $log }

Log "=== nightly ingest START ==="

$bin = ".\target\release"
# Order matters: attribution runs LAST — it links the freshly-synced Quicket
# sales to the freshly-synced Meta Ads campaigns.
$steps = @(
  @{ name = 'instagram';   exe = "$bin\ingest_instagram.exe";   args = @('all') },
  @{ name = 'meta_ads';    exe = "$bin\ingest_metaads.exe";     args = @() },
  @{ name = 'quicket';     exe = "$bin\ingest.exe";             args = @('sales') },
  @{ name = 'yoco';        exe = "$bin\ingest_yoco.exe";        args = @() },
  @{ name = 'artists';     exe = "$bin\ingest_artists.exe";     args = @() },
  @{ name = 'attribution'; exe = "$bin\ingest_attribution.exe"; args = @() }
)

foreach ($s in $steps) {
  Log ("--- {0} start ---" -f $s.name)
  try {
    & $s.exe @($s.args) *>> $log
    if ($LASTEXITCODE -eq 0) { Log ("--- {0} OK ---" -f $s.name) }
    else { Log ("--- {0} FAILED exit {1} ---" -f $s.name, $LASTEXITCODE) }
  } catch {
    Log ("--- {0} ERROR: {1} ---" -f $s.name, $_.Exception.Message)
  }
}

Log "=== nightly ingest DONE ==="
