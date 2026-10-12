# Delete the "Ubuntu" WSL distro so Mini-Me's Setup installs the backend from scratch.
#
# Run from PowerShell (administrator is fine) as the same Windows user who uses the app. WSL
# distros belong to a Windows account, so another account would not see this one:
#
#   powershell -ExecutionPolicy Bypass -File .\reset-mini-me-backend.ps1
#
# Removes ONLY the Ubuntu distro and everything inside it:
#   - the backend and its Python environment
#   - the conversation history the app lists (it is stored in the backend's database)
#   - the Asta CLI and its sign-in, and the CIP Dataverse sign-in
#   - anything else you or anyone put in that Ubuntu
#
# Does NOT uninstall WSL, the Windows features it needs, or any other distro (docker-desktop and
# the like are left alone). Mini-Me itself, its settings and Documents\Mini-Me are untouched.
#
# Afterwards, open Mini-Me: Setup sees that Ubuntu is missing and installs it again.

param(
    [switch]$Yes
)

$ErrorActionPreference = 'Stop'
$distro = 'Ubuntu'

$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$signedIn = (Get-CimInstance Win32_ComputerSystem).UserName
if ($signedIn -and $signedIn -ne $identity.Name) {
    Write-Host "PowerShell is running as $($identity.Name), but $signedIn is signed in." -ForegroundColor Red
    Write-Host 'WSL distros are per account. Open PowerShell from the account that uses Mini-Me.' -ForegroundColor Red
    exit 1
}

if (-not (Get-Command wsl.exe -ErrorAction SilentlyContinue)) {
    Write-Host 'WSL is not installed, so there is no backend to remove.'
    exit 0
}

# `wsl -l -q` prints UTF-16, which arrives here with a NUL between characters.
$distros = (& wsl.exe --list --quiet 2>$null) | ForEach-Object { ($_ -replace "`0", '').Trim() } | Where-Object { $_ }
if ($distros -notcontains $distro) {
    Write-Host "There is no '$distro' distro in WSL. Nothing to remove."
    if ($distros) { Write-Host ("Distros present (left alone): " + ($distros -join ', ')) }
    exit 0
}

Write-Host "This permanently deletes the WSL distro '$distro' and everything inside it," -ForegroundColor Yellow
Write-Host 'including Mini-Me''s backend and its conversation history.' -ForegroundColor Yellow
$others = $distros | Where-Object { $_ -ne $distro }
if ($others) { Write-Host ("Other distros are kept: " + ($others -join ', ')) }
if (-not $Yes) {
    $answer = Read-Host "Type the distro name ($distro) to confirm"
    if ($answer -cne $distro) { Write-Host 'Nothing was changed.'; exit 0 }
}

$running = Get-Process -Name 'mini-me-desktop-app' -ErrorAction SilentlyContinue
if ($running) {
    Write-Host 'Closing Mini-Me...'
    $running | Stop-Process -Force
    Start-Sleep -Seconds 2
}

& wsl.exe --terminate $distro 2>$null | Out-Null
& wsl.exe --unregister $distro
if ($LASTEXITCODE -ne 0) {
    Write-Host "wsl --unregister $distro failed (exit $LASTEXITCODE). Nothing else was changed." -ForegroundColor Red
    exit 1
}

Write-Host ''
Write-Host "The '$distro' distro is gone." -ForegroundColor Green
Write-Host 'Open Mini-Me and follow Setup to install the backend again.'
