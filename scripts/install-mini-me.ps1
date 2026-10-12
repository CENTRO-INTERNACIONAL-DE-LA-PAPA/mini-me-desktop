# Install (or update by hand) Mini-Me into C:\Program Files\mini-me-desktop.
#
# Run from PowerShell opened with "Run as administrator", as the same Windows user who will
# use the app:
#
#   powershell -ExecutionPolicy Bypass -File .\install-mini-me.ps1
#   powershell -ExecutionPolicy Bypass -File .\install-mini-me.ps1 -Zip "D:\mini-me-desktop-v0.3.47-windows-x64.zip"
#
# With no -Zip it takes the newest mini-me-desktop-*-windows-x64.zip in your Downloads folder.
#
# Why Program Files: IT whitelists the app by that path. Why Move-Item and not a copy: a moved
# folder keeps the user's Full control from Downloads, and the self-updater replaces files inside
# the install folder in place. A copied folder inherits Program Files' read-only permissions and
# every later update fails. The script also grants the user Modify explicitly, so the install
# still updates when Downloads is on another drive and the move turns into a copy.
#
# Conversations, files and settings live outside Program Files, so replacing an existing install
# keeps them.

param(
    [string]$Zip
)

$ErrorActionPreference = 'Stop'
$installDir = 'C:\Program Files\mini-me-desktop'
$exeName = 'mini-me-desktop-app.exe'

$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
if (-not ([Security.Principal.WindowsPrincipal]$identity).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Write-Host 'This script needs PowerShell opened with "Run as administrator".' -ForegroundColor Red
    exit 1
}

# The person who will use the app. When PowerShell was elevated with a different (admin) account,
# $env:USERPROFILE points at that account's Downloads, and the permission grant must go to the
# signed-in user rather than to the admin.
$appUser = (Get-CimInstance Win32_ComputerSystem).UserName
if (-not $appUser) { $appUser = $identity.Name }
if ($appUser -ne $identity.Name -and -not $Zip) {
    Write-Host "PowerShell is running as $($identity.Name), but $appUser is signed in." -ForegroundColor Yellow
    Write-Host 'Pass the zip explicitly with -Zip "<path>" so the right file is used.' -ForegroundColor Yellow
    exit 1
}

if (-not $Zip) {
    $found = Get-ChildItem "$env:USERPROFILE\Downloads\mini-me-desktop-*-windows-x64.zip" -ErrorAction SilentlyContinue |
        Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if (-not $found) {
        Write-Host "No mini-me-desktop-*-windows-x64.zip in $env:USERPROFILE\Downloads." -ForegroundColor Red
        Write-Host 'Download it there, or pass its location with -Zip "<path>".' -ForegroundColor Red
        exit 1
    }
    $Zip = $found.FullName
}
if (-not (Test-Path -LiteralPath $Zip)) {
    Write-Host "Zip not found: $Zip" -ForegroundColor Red
    exit 1
}
Write-Host "Installing from $Zip"

# Unpack next to the zip, not into Program Files, so the folder starts out with the user's
# permissions and then moves.
$unzipped = Join-Path (Split-Path -Parent $Zip) 'mini-me-desktop-install'
if (Test-Path -LiteralPath $unzipped) { Remove-Item -LiteralPath $unzipped -Recurse -Force }
Expand-Archive -LiteralPath $Zip -DestinationPath $unzipped
$newBuild = Join-Path $unzipped 'mini-me-desktop'
if (-not (Test-Path -LiteralPath (Join-Path $newBuild $exeName))) {
    Remove-Item -LiteralPath $unzipped -Recurse -Force
    Write-Host "The zip does not contain mini-me-desktop\$exeName. Is it a Mini-Me release?" -ForegroundColor Red
    exit 1
}

$running = Get-Process -Name 'mini-me-desktop-app' -ErrorAction SilentlyContinue
if ($running) {
    Write-Host 'Closing Mini-Me...'
    $running | Stop-Process -Force
    Start-Sleep -Seconds 2
}

if (Test-Path -LiteralPath $installDir) {
    Write-Host 'Replacing the existing install (conversations and settings are kept).'
    Remove-Item -LiteralPath $installDir -Recurse -Force
}
Move-Item -LiteralPath $newBuild -Destination $installDir
Remove-Item -LiteralPath $unzipped -Recurse -Force

# (OI)(CI)M = Modify on the folder and everything in it, inherited by files the updater creates.
& icacls.exe $installDir /grant "${appUser}:(OI)(CI)M" /T /C /Q | Out-Null
if ($LASTEXITCODE -ne 0) {
    Write-Host "Installed, but could not give $appUser permission to update it (icacls exit $LASTEXITCODE)." -ForegroundColor Yellow
}

Write-Host ''
Write-Host "Mini-Me installed from $(Split-Path -Leaf $Zip)" -ForegroundColor Green
Write-Host "Open it from $installDir\$exeName"
