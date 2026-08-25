# Rehearse the update swap, against folders that do not matter.
#
# Seven bugs into the update flow (docs 268-272) every one has lived in the seam between
# the app and Windows, and none of them can be executed from the machine the app is
# written on. This runs the app's own swap script — verbatim, through -EncodedCommand,
# exactly as `update::begin_swap` runs it — against a throwaway install under %TEMP%.
#
# Nothing real is touched. It answers in seconds what a pair of releases answers in an
# hour.
#
#   irm https://raw.githubusercontent.com/CENTRO-INTERNACIONAL-DE-LA-PAPA/mini-me-desktop/main/scripts/swap-rehearsal.ps1 -OutFile "$env:TEMP\swap-rehearsal.ps1"
#   powershell -ExecutionPolicy Bypass -File "$env:TEMP\swap-rehearsal.ps1"

$ErrorActionPreference = 'Continue'
$base = Join-Path $env:TEMP 'swap-rehearsal'
$out  = Join-Path $env:TEMP 'swap-rehearsal.out'
$err  = Join-Path $env:TEMP 'swap-rehearsal.err'

Write-Host "=== a throwaway install under $base ===" -ForegroundColor Cyan
Remove-Item -Recurse -Force $base -ErrorAction SilentlyContinue
Remove-Item -Force $out, $err -ErrorAction SilentlyContinue
foreach ($d in @(
    'mini-me-desktop\overlay', 'mini-me-desktop\scripts', 'mini-me-desktop\vendor',
    '.mini-me-update-9.9.9\mini-me-desktop\overlay',
    '.mini-me-update-9.9.9\mini-me-desktop\scripts',
    '.mini-me-update-9.9.9\mini-me-desktop\vendor')) {
  New-Item -ItemType Directory -Force -Path (Join-Path $base $d) | Out-Null
}
Set-Content (Join-Path $base 'mini-me-desktop\mini-me-desktop-app.exe') 'THE OLD BUILD'
Set-Content (Join-Path $base '.mini-me-update-9.9.9\mini-me-desktop\mini-me-desktop-app.exe') 'THE NEW BUILD'
Write-Host ("before: " + (Get-Content (Join-Path $base 'mini-me-desktop\mini-me-desktop-app.exe')))

# The app's own script. Generated from `update::swap_script`, with only the three paths
# substituted — everything else is character for character what the app sends.
$script = @'
$ErrorActionPreference = 'Stop'; function Note($m) { Write-Output "$(Get-Date -Format o) $m" }; Note 'waiting for mini-me-desktop-app (pid 999999) to exit'; $left = 60; while ($left -gt 0 -and (Get-Process -Id 999999 -ErrorAction SilentlyContinue)) { Start-Sleep -Seconds 1; $left-- }; if (Get-Process -Id 999999 -ErrorAction SilentlyContinue) { Note 'it is still running after 60s, so nothing was changed'; exit 1 }; Start-Sleep -Milliseconds 750; try { if (Test-Path -LiteralPath '__BASE__/.mini-me-previous-9.9.9') { Remove-Item -LiteralPath '__BASE__/.mini-me-previous-9.9.9' -Recurse -Force }; Move-Item -LiteralPath '__BASE__/mini-me-desktop' -Destination '__BASE__/.mini-me-previous-9.9.9' -Force; Note 'retired the old folder' } catch { Note "could not move the old folder aside: $_"; exit 1 }; try { Move-Item -LiteralPath '__BASE__/.mini-me-update-9.9.9/mini-me-desktop' -Destination '__BASE__/mini-me-desktop' -Force; Note 'the new build is in place' } catch { Note "could not move the new build in, putting the old one back: $_"; Move-Item -LiteralPath '__BASE__/.mini-me-previous-9.9.9' -Destination '__BASE__/mini-me-desktop' -Force; Note 'the old build is back'; exit 1 }; try { Start-Process -FilePath '__BASE__/mini-me-desktop/mini-me-desktop-app.exe' -WorkingDirectory '__BASE__/mini-me-desktop'; Note 'relaunched' } catch { Note "the new build is in place but did not start: $_" }; Remove-Item -LiteralPath '__BASE__/.mini-me-previous-9.9.9' -Recurse -Force -ErrorAction SilentlyContinue; Remove-Item -LiteralPath '__BASE__/.mini-me-update-9.9.9' -Recurse -Force -ErrorAction SilentlyContinue; Note 'done'
'@
$script = $script.Replace('__BASE__', $base).Replace('__LOG__', $out).Replace('__WORK__', $env:TEMP)
$encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($script))
Write-Host ("script: {0} chars, encoded: {1} chars" -f $script.Length, $encoded.Length)

Write-Host "`n=== running it the way the app does ===" -ForegroundColor Cyan
$p = Start-Process -FilePath 'powershell' `
  -ArgumentList '-NoProfile','-NonInteractive','-WindowStyle','Hidden','-EncodedCommand',$encoded `
  -WorkingDirectory $env:TEMP -PassThru -Wait `
  -RedirectStandardError $err -RedirectStandardOutput $out
Write-Host "exit code: $($p.ExitCode)"

# The helper reports on **stdout**, which the app redirects into the update log. It must not
# open that file itself: it already holds it, and a second open fails with a sharing violation
# that killed the script on its first statement three times over (docs 274).
Write-Host "`n=== what the helper reported ===" -ForegroundColor Cyan
if ((Test-Path $out) -and (Get-Item $out).Length -gt 0) { Get-Content $out }
else { Write-Host 'NOTHING — the helper said nothing at all' -ForegroundColor Red }

if ((Test-Path $err) -and (Get-Item $err).Length -gt 0) {
  Write-Host "`n=== what PowerShell put on stderr ===" -ForegroundColor Yellow
  Get-Content $err
}

Write-Host "`n=== did the folder move? ===" -ForegroundColor Cyan
$app = Join-Path $base 'mini-me-desktop\mini-me-desktop-app.exe'
if (Test-Path $app) {
  $now = Get-Content $app
  Write-Host "after : $now"
  if ($now -eq 'THE NEW BUILD') { Write-Host 'SWAP WORKED' -ForegroundColor Green }
  else { Write-Host 'THE OLD BUILD IS STILL THERE' -ForegroundColor Red }
} else {
  Write-Host 'THE INSTALL FOLDER IS GONE — the bad case' -ForegroundColor Red
}

Write-Host "`n(the relaunch step is expected to fail here: the fake .exe is a text file)" -ForegroundColor DarkGray
