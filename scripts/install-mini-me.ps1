# Install (or update by hand) Mini-Me into C:\Program Files\mini-me-desktop.
#
# This script ships inside the release bundle, at mini-me-desktop\scripts\. Unzip the release,
# then run it from PowerShell opened with "Run as administrator", as the same Windows user who
# will use the app:
#
#   powershell -ExecutionPolicy Bypass -File "$env:USERPROFILE\Downloads\mini-me-desktop\scripts\install-mini-me.ps1"
#
# It installs the mini-me-desktop folder it sits in. -Source points it at another unzipped
# bundle instead (for example when running the copy in the repository):
#
#   powershell -ExecutionPolicy Bypass -File .\install-mini-me.ps1 -Source "D:\mini-me-desktop"
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
    [string]$Source
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
# the permission grant must go to the signed-in user rather than to the admin.
$appUser = (Get-CimInstance Win32_ComputerSystem).UserName
if (-not $appUser) { $appUser = $identity.Name }

# Default: the bundle this script came in (its parent folder, since it lives in scripts\).
if (-not $Source) { $Source = Split-Path -Parent $PSScriptRoot }
if (-not (Test-Path -LiteralPath (Join-Path $Source $exeName))) {
    Write-Host "No $exeName in $Source." -ForegroundColor Red
    Write-Host 'Run this script from inside the unzipped mini-me-desktop folder, or pass -Source "<folder>".' -ForegroundColor Red
    exit 1
}
$newBuild = (Resolve-Path -LiteralPath $Source).Path.TrimEnd('\')
if ($newBuild -ieq $installDir) {
    Write-Host "This copy is already installed in $installDir. Run the script from a newly unzipped bundle to update." -ForegroundColor Yellow
    exit 1
}
Write-Host "Installing from $newBuild"

# Step out of the bundle so this session doesn't hold the folder that is about to move.
Set-Location -LiteralPath $env:SystemRoot

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

# (OI)(CI)M = Modify on the folder and everything in it, inherited by files the updater creates.
& icacls.exe $installDir /grant "${appUser}:(OI)(CI)M" /T /C /Q | Out-Null
if ($LASTEXITCODE -ne 0) {
    Write-Host "Installed, but could not give $appUser permission to update it (icacls exit $LASTEXITCODE)." -ForegroundColor Yellow
}

Write-Host ''
Write-Host "Mini-Me installed from $newBuild" -ForegroundColor Green
Write-Host "Open it from $installDir\$exeName"
