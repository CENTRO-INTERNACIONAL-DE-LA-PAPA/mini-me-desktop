# Provision the Mini-Me backend for the desktop app, natively on Windows.
#
# No WSL: the backend runs on this machine directly (`.venv\Scripts\langgraph.exe`),
# and Asta is a normal Python dependency of this checkout (a path dependency on the
# `asta-plugins` submodule, resolved by `uv sync` below) rather than a separately
# installed CLI reached inside a Linux distro. See `scripts/setup-backend.sh` for the
# macOS/Linux equivalent — the two are kept in step; a change to one almost always
# belongs in the other too.
#
# The desktop app runs this itself (via `powershell.exe`, never `wsl.exe`), from the
# Setup pane, and shows the output line by line. So every message here is written for
# someone who does not write code: no unexplained jargon, no step that ends in "now go
# and edit this file".
#
# Safe to re-run. It never overwrites a checkout it did not create.
#
#   Usage:  powershell -File setup-backend.ps1 [-Dir <target-dir>]
#           default target: $env:LOCALAPPDATA\mini-me-desktop\backend
#           asta-plugins lands as a SIBLING of -Dir (..\asta-plugins), because that is
#           the relative path mini-me/pyproject.toml's `[tool.uv.sources]` expects.
#
#   $env:MINIME_BUNDLED_SOURCE  a Mini-Me copy shipped with the app, to copy from
#                               instead of cloning
#   $env:MINIME_REPO_URL        override the Mini-Me git remote
#   $env:ASTA_BUNDLED_SOURCE    an asta-plugins copy shipped with the app, to copy from
#                               instead of cloning
#   $env:ASTA_REPO_URL          override the asta-plugins git remote

param(
    [string]$Dir = "$env:LOCALAPPDATA\mini-me-desktop\backend"
)

$ErrorActionPreference = "Stop"

$RepoUrl = if ($env:MINIME_REPO_URL) { $env:MINIME_REPO_URL } else { "https://github.com/CENTRO-INTERNACIONAL-DE-LA-PAPA/Mini-Me.git" }
$AstaRepoUrl = if ($env:ASTA_REPO_URL) { $env:ASTA_REPO_URL } else { "git@github.com-cip:allenai/asta-plugins.git" }
$AstaDir = Join-Path (Split-Path -Parent $Dir) "asta-plugins"
$Here = Split-Path -Parent $MyInvocation.MyCommand.Path

function Say($msg) { Write-Host "`n==> $msg" }
function Ok($msg) { Write-Host "    ok  $msg" }
function Bad($msg) { Write-Host "    !!  $msg" }

# ------------------------------------------------------------------------ tools
Say "Checking what is already installed"
if (-not (Get-Command git -ErrorAction SilentlyContinue)) {
    Bad "git is missing. Install it from https://git-scm.com/download/win"
    exit 1
}
Ok "git $((git --version) -replace '^git version ')"

if (-not (Get-Command uv -ErrorAction SilentlyContinue)) {
    Say "Installing uv (the Python package manager)"
    Invoke-Expression (Invoke-RestMethod https://astral.sh/uv/install.ps1)
    # Visible to the rest of *this* script, too — the installer only updates the
    # registry PATH, which new processes pick up but this one already has open.
    $localBin = Join-Path $env:USERPROFILE ".local\bin"
    if (Test-Path $localBin) { $env:PATH = "$localBin;$env:PATH" }
}
Ok "uv $((uv --version) -replace '^uv ')"

# ------------------------------------------------------------- generic checkout
#
# Both Mini-Me and asta-plugins are private repositories bundled with the app so a
# from-scratch install needs no GitHub credentials (whoever builds the installer runs
# scripts/bundle-backend.sh once, and that path is free). In preference order:
#
#   1. A copy bundled with the app.
#   2. A checkout already on this machine — copied, not downloaded again.
#   3. git clone. The developer path, and the only one that can ask for a
#      password, which is why it is last.
function Find-CheckoutSource {
    param([string]$Marker, [string]$TargetDir, [string]$Bundled, [string[]]$Candidates)
    if ($Bundled -and (Test-Path (Join-Path $Bundled $Marker))) {
        return $Bundled
    }
    foreach ($candidate in $Candidates) {
        if ($candidate -and (Test-Path $candidate) -and (Test-Path (Join-Path $candidate $Marker))) {
            $resolved = (Resolve-Path $candidate).Path
            if ($resolved -ne (Resolve-Path -LiteralPath $TargetDir -ErrorAction SilentlyContinue).Path) {
                return $resolved
            }
        }
    }
    return $null
}

# The stamp `scripts/package.sh` writes into a bundled copy. Empty for a developer
# checkout, which is the signal that this source is somebody's working tree and must
# not be copied over anything.
function Stamp-Of([string]$path) {
    $stampFile = Join-Path $path ".bundled-backend"
    if (Test-Path $stampFile) { return (Get-Content $stampFile -Raw).Trim() }
    return ""
}

function Provision-Checkout {
    param(
        [string]$Name, [string]$TargetDir, [string]$Marker,
        [string]$Bundled, [string]$RepoUrl, [string[]]$Candidates
    )
    Say "Checking $Name ($TargetDir)"
    $markerPath = Join-Path $TargetDir $Marker
    if (Test-Path $markerPath) {
        # **Already installed is not the same as up to date.** Only overwrite when the
        # source carries a stamp, so a developer checkout adopted with "use the one I
        # have" is never overwritten — it may hold real work.
        $source = Find-CheckoutSource -Marker $Marker -TargetDir $TargetDir -Bundled $Bundled -Candidates $Candidates
        $updated = $false
        if ($source) {
            $bundledStamp = Stamp-Of $source
            $installedStamp = Stamp-Of $TargetDir
            if ($bundledStamp -and ($bundledStamp -ne $installedStamp)) {
                Say "Updating $Name from the copy bundled with this app"
                Write-Host "    installed $(if ($installedStamp) { $installedStamp } else { 'unstamped' }) -> bundled $($bundledStamp.Substring(0, [Math]::Min(12, $bundledStamp.Length)))"
                Get-ChildItem -Path $TargetDir -Force | Where-Object {
                    $_.Name -notin @(".venv", ".env", ".git", ".desktop-overlay", ".langgraph_api")
                } | Remove-Item -Recurse -Force
                Copy-Item -Path (Join-Path $source "*") -Destination $TargetDir -Recurse -Force -Exclude @(".git")
                if (-not (Test-Path $markerPath)) {
                    Bad "the update did not bring $Marker — $source may be incomplete"
                    exit 1
                }
                $updated = $true
                Ok "updated from $source"
            }
        }
        if (-not $updated) { Ok "$Name is already here and current: $TargetDir" }
        return
    }

    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $TargetDir) | Out-Null
    if ((Test-Path $TargetDir) -and ((Get-ChildItem $TargetDir -Force | Measure-Object).Count -eq 0)) {
        Remove-Item $TargetDir -Force
    }

    $source = Find-CheckoutSource -Marker $Marker -TargetDir $TargetDir -Bundled $Bundled -Candidates $Candidates
    if ($source) {
        Say "Copying $Name from $source"
        Write-Host "    (faster than downloading, and it needs no password)"
        New-Item -ItemType Directory -Force -Path $TargetDir | Out-Null
        Copy-Item -Path (Join-Path $source "*") -Destination $TargetDir -Recurse -Force
        if (-not (Test-Path $markerPath)) {
            Bad "the copy did not bring $Marker — $source may be incomplete"
            exit 1
        }
        # A copied .venv holds the *other* machine's compiled packages. Unusable here.
        $venv = Join-Path $TargetDir ".venv"
        if (Test-Path $venv) {
            Remove-Item $venv -Recurse -Force
            Ok "removed the copied environment (this machine needs to build its own)"
        }
        Ok "copied to $TargetDir"
    } else {
        Say "Downloading $Name"
        Write-Host "    This is a private repository, so git will ask who you are."
        Write-Host "    If you are reading this inside the app, $Name was not bundled"
        Write-Host "    with your copy: ask whoever gave it to you."
        git clone $RepoUrl $TargetDir
    }
}

Provision-Checkout -Name "Mini-Me" -TargetDir $Dir -Marker "langgraph.json" `
    -Bundled $env:MINIME_BUNDLED_SOURCE -RepoUrl $RepoUrl `
    -Candidates @("$env:USERPROFILE\Documents\Mini-Me", "$env:USERPROFILE\Documents\GitHub\Mini-Me")

Provision-Checkout -Name "asta-plugins" -TargetDir $AstaDir -Marker "pyproject.toml" `
    -Bundled $env:ASTA_BUNDLED_SOURCE -RepoUrl $AstaRepoUrl `
    -Candidates @("$env:USERPROFILE\Documents\asta-plugins")

Set-Location $Dir

# ------------------------------------------------------------------ the overlay
#
# Copied *into the checkout* rather than left wherever this script lives. Host
# execution works by putting this directory on the backend's PYTHONPATH, and a path
# next to the app's own install is reachable only while the app's own folder still
# exists. Three small files; copying them removes a whole class of silent failure.
$OverlaySrc = Join-Path $Here "..\overlay"
if (Test-Path (Join-Path $OverlaySrc "sitecustomize.py")) {
    Say "Installing the local-execution overlay"
    $overlayDest = Join-Path $Dir ".desktop-overlay"
    if (Test-Path $overlayDest) { Remove-Item $overlayDest -Recurse -Force }
    Copy-Item -Path $OverlaySrc -Destination $overlayDest -Recurse -Force
    Get-ChildItem -Path $overlayDest -Filter "__pycache__" -Recurse -Directory -ErrorAction SilentlyContinue |
        Remove-Item -Recurse -Force -ErrorAction SilentlyContinue
    Ok "overlay installed here"
} else {
    Bad "could not find the overlay next to this script — the app will use its own copy"
}

# **A stopping point, so the step above can be rehearsed.**
if ($env:MINIME_SETUP_STOP_AFTER_SOURCE) {
    Say "Stopping after the source step (MINIME_SETUP_STOP_AFTER_SOURCE)"
    exit 0
}

# ------------------------------------------------------------------ dependencies
# --extra dev is REQUIRED: langgraph-cli lives in an optional extra, so a plain
# `uv sync` leaves you with the server libraries but no `langgraph` entry point.
# This also pulls in `asta` itself, per `[tool.uv.sources]` in pyproject.toml, from
# the sibling checkout provisioned above — no separate CLI install step.
Say "Installing Python packages (a few minutes the first time)"
Write-Host "    This pulls the scientific stack — PyMC, scikit-learn and friends — and Asta."
uv sync --extra dev

# Durable conversation storage, installed by default and not left to a checkbox.
Say "Installing durable conversation storage"
uv pip install langgraph-checkpoint-sqlite
if ($LASTEXITCODE -ne 0) {
    Bad "could not install langgraph-checkpoint-sqlite - Setup will offer it again"
}

if (Test-Path ".venv\Scripts\langgraph.exe") {
    Ok "the backend can be started"
} else {
    Bad ".venv\Scripts\langgraph.exe is missing even after --extra dev."
    Bad "The desktop app cannot start the backend without it."
    exit 1
}

# -------------------------------------------------------------------------- env
# Deliberately NOT a key template. Keys live in the desktop app's settings panel and
# travel with each request from the OS keychain, so nobody has to edit a file to get
# started. An empty .env is still written because `langgraph dev` auto-loads one when
# present, and its absence has made people think they missed a step.
if (-not (Test-Path ".env")) {
    @"
# Nothing needs to go in here.
#
# Your API keys live in the desktop app: open Settings and paste them there. They
# are kept in your operating system's keychain and sent with each request, so they
# are never written to a file on disk.
"@ | Set-Content -Path ".env" -NoNewline
    Ok "wrote an (intentionally empty) .env"
}

# ------------------------------------------------------------------------- done
Say "Done - Mini-Me is ready."
Write-Host "    Location: $Dir"
Write-Host ""
Write-Host "    Back in the app, press Re-check. If a model key is still missing, open"
Write-Host "    Settings and paste one. Nothing else needs doing here."
