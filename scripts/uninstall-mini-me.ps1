# Uninstall Mini-Me from this Windows account.
#
# Run from PowerShell opened with "Run as administrator", as the same Windows user who uses the
# app (settings, keys and app data are per user):
#
#   powershell -ExecutionPolicy Bypass -File .\uninstall-mini-me.ps1
#   powershell -ExecutionPolicy Bypass -File .\uninstall-mini-me.ps1 -RemoveConversations
#
# Removes:
#   - the app:            C:\Program Files\mini-me-desktop
#   - settings:           %APPDATA%\mini-me-desktop      (settings.toml, models.json)
#   - local app data:     %LOCALAPPDATA%\mini-me-desktop
#   - saved keys:         Credential Manager entries ending in .mini-me-desktop (model keys, Asta token)
#   - logs:               %TEMP%\mini-me-desktop-*.log
#   - shortcuts to the app on the Desktop and in the Start menu
#
# Keeps, unless asked:
#   - Documents\Mini-Me (the files of every conversation). -RemoveConversations deletes it too.
#
# Never touches WSL. The backend and the conversation history live in the "Ubuntu" distro; remove
# them with reset-mini-me-backend.ps1.

param(
    [switch]$RemoveConversations,
    [switch]$Yes
)

$ErrorActionPreference = 'Stop'
$installDir = 'C:\Program Files\mini-me-desktop'
$conversations = Join-Path ([Environment]::GetFolderPath('MyDocuments')) 'Mini-Me'

$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
if (-not ([Security.Principal.WindowsPrincipal]$identity).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Write-Host 'This script needs PowerShell opened with "Run as administrator".' -ForegroundColor Red
    exit 1
}
# Keys, settings and app data belong to the signed-in user. Elevated as another account, this
# would clean that account instead and report success.
$signedIn = (Get-CimInstance Win32_ComputerSystem).UserName
if ($signedIn -and $signedIn -ne $identity.Name) {
    Write-Host "PowerShell is running as $($identity.Name), but $signedIn is signed in." -ForegroundColor Red
    Write-Host 'Open PowerShell as administrator from the account that uses Mini-Me.' -ForegroundColor Red
    exit 1
}

Write-Host 'This removes Mini-Me, its settings and its saved keys from this account.'
if ($RemoveConversations) {
    Write-Host "It also DELETES $conversations (every conversation's files)." -ForegroundColor Yellow
} else {
    Write-Host "Conversation files in $conversations are kept."
}
if (-not $Yes) {
    $answer = Read-Host 'Continue? (y/n)'
    if ($answer -notmatch '^(y|yes)$') { Write-Host 'Nothing was changed.'; exit 0 }
}

$running = Get-Process -Name 'mini-me-desktop-app' -ErrorAction SilentlyContinue
if ($running) {
    Write-Host 'Closing Mini-Me...'
    $running | Stop-Process -Force
    Start-Sleep -Seconds 2
}

function Remove-IfPresent([string]$Path, [string]$What) {
    if (Test-Path -LiteralPath $Path) {
        Remove-Item -LiteralPath $Path -Recurse -Force
        Write-Host "Removed $What ($Path)"
    }
}

# Shortcuts first, while their target still exists to compare against.
$shell = New-Object -ComObject WScript.Shell
$shortcutDirs = @(
    [Environment]::GetFolderPath('Desktop'),
    [Environment]::GetFolderPath('CommonDesktopDirectory'),
    [Environment]::GetFolderPath('StartMenu'),
    [Environment]::GetFolderPath('CommonStartMenu')
) | Where-Object { $_ -and (Test-Path -LiteralPath $_) }
foreach ($dir in $shortcutDirs) {
    Get-ChildItem -LiteralPath $dir -Filter *.lnk -Recurse -ErrorAction SilentlyContinue | ForEach-Object {
        $target = $shell.CreateShortcut($_.FullName).TargetPath
        if ($target -and $target.StartsWith($installDir, [StringComparison]::OrdinalIgnoreCase)) {
            Remove-Item -LiteralPath $_.FullName -Force
            Write-Host "Removed shortcut $($_.FullName)"
        }
    }
}

Remove-IfPresent $installDir 'the app'
Remove-IfPresent (Join-Path $env:APPDATA 'mini-me-desktop') 'settings'
Remove-IfPresent (Join-Path $env:LOCALAPPDATA 'mini-me-desktop') 'local app data'
Get-ChildItem -Path (Join-Path $env:TEMP 'mini-me-desktop-*.log') -ErrorAction SilentlyContinue |
    ForEach-Object { Remove-Item -LiteralPath $_.FullName -Force; Write-Host "Removed log $($_.Name)" }

# The keyring crate stores each secret as a generic credential named "<name>.mini-me-desktop".
$targets = & cmdkey.exe /list | Select-String 'mini-me-desktop' | ForEach-Object {
    ($_.Line -replace '^\s*Target:\s*', '' -replace '^LegacyGeneric:target=', '').Trim()
}
foreach ($target in $targets) {
    & cmdkey.exe "/delete:$target" | Out-Null
    Write-Host "Removed saved key $target"
}

if ($RemoveConversations) { Remove-IfPresent $conversations 'conversation files' }

Write-Host ''
Write-Host 'Mini-Me is uninstalled.' -ForegroundColor Green
Write-Host 'The backend in WSL (Ubuntu) is untouched. To remove it too, run reset-mini-me-backend.ps1.'
