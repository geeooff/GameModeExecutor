# Registers the two scheduled tasks this recipe needs, filling in the
# placeholders for you.
#
# RUN THIS FROM AN ELEVATED POWERSHELL.
#
# FanControl requires administrator rights because it talks to hardware, and
# GameModeExecutor runs without them on purpose, so it cannot start FanControl
# directly. The bridge is one scheduled task per profile, registered once with
# "run with highest privileges". Triggering one afterwards needs no rights and
# raises no prompt.
#
# Usage:
#   .\install-tasks.ps1
#   .\install-tasks.ps1 -FanControlDir "D:\Tools\FanControl"   (if detection fails)
#   .\install-tasks.ps1 -Profiles Game,Quiet,Pump              (to add your own)

param(
    [string]   $FanControlDir,
    [string[]] $Profiles = @('Game', 'Quiet')
)

$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path

# --- 1. Refuse now rather than half-way through -----------------------------
$admin = ([Security.Principal.WindowsPrincipal] [Security.Principal.WindowsIdentity]::GetCurrent()
         ).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $admin) {
    Write-Host "This needs an elevated PowerShell." -ForegroundColor Red
    Write-Host "Start menu -> type PowerShell -> right-click -> Run as administrator."
    Write-Host "Nothing was changed."
    exit 1
}

# --- 2. Find FanControl -----------------------------------------------------
# It has no standard install folder: it ships as an archive you extract
# wherever you like.
if (-not $FanControlDir) {
    $found = @(
        (Get-Process FanControl -ErrorAction SilentlyContinue | Select-Object -First 1).Path,
        "$env:LOCALAPPDATA\Programs\FanControl\FanControl.exe",
        "$env:ProgramFiles\FanControl\FanControl.exe",
        "${env:ProgramFiles(x86)}\FanControl\FanControl.exe",
        "$env:USERPROFILE\scoop\apps\fancontrol\current\FanControl.exe"
    ) | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1

    if ($found) { $FanControlDir = Split-Path $found }
}

if (-not $FanControlDir -or -not (Test-Path (Join-Path $FanControlDir 'FanControl.exe'))) {
    Write-Host "FanControl.exe not found." -ForegroundColor Red
    Write-Host "Run again naming its folder, for example:"
    Write-Host '   .\install-tasks.ps1 -FanControlDir "D:\Tools\FanControl"'
    Write-Host "(right-click your FanControl shortcut -> Open file location)"
    exit 1
}
Write-Host "FanControl : $FanControlDir" -ForegroundColor Cyan

# --- 3. Check the profiles exist --------------------------------------------
# They cannot be shipped: a fan curve depends on the machine's hardware, so
# someone else's would be useless at best.
$configs = Join-Path $FanControlDir 'Configurations'
$missing = $Profiles | Where-Object { -not (Test-Path (Join-Path $configs "$_.json")) }
if ($missing) {
    Write-Host ""
    Write-Host "Missing profiles in ${configs}: $(($missing | ForEach-Object { "$_.json" }) -join ', ')" -ForegroundColor Yellow
    Write-Host "Create them in FanControl (set the curves, then Save configuration as...)."
    Write-Host "The tasks are registered anyway; they will do nothing until the"
    Write-Host "profiles exist."
    Write-Host ""
}

# --- 4. Register ------------------------------------------------------------
$user = "$env:USERDOMAIN\$env:USERNAME"
Write-Host "Account    : $user" -ForegroundColor Cyan
Write-Host ""

# Any profile beyond Game and Quiet reuses the Game template: the two differ
# only in the argument and the description.
foreach ($profile in $Profiles) {
    $template = Join-Path $here "FanControl-$profile.xml"
    if (-not (Test-Path $template)) { $template = Join-Path $here "FanControl-Game.xml" }
    if (-not (Test-Path $template)) {
        Write-Host "  no template found for $profile" -ForegroundColor Red
        continue
    }

    $xml = [System.IO.File]::ReadAllText($template, [System.Text.Encoding]::Unicode)
    $xml = $xml.Replace('__FANCONTROL_DIR__', $FanControlDir)
    $xml = $xml.Replace('__DOMAIN__\__USERNAME__', $user)
    # Harmless when the template already matches; needed when reusing Game's.
    $xml = $xml -replace '-c \w+\.json', "-c $profile.json"
    $xml = $xml -replace '<URI>[^<]*</URI>', "<URI>\GameModeExecutor\FanControl $profile</URI>"
    $xml = $xml -replace 'the FanControl &quot;\w+&quot; profile', "the FanControl &quot;$profile&quot; profile"

    Register-ScheduledTask -Xml $xml -TaskName "FanControl $profile" `
                           -TaskPath '\GameModeExecutor\' -Force | Out-Null
    Write-Host "  registered \GameModeExecutor\FanControl $profile" -ForegroundColor Green
}

# --- 5. Show what landed ----------------------------------------------------
Write-Host ""
Write-Host "In \GameModeExecutor :" -ForegroundColor Cyan
Get-ScheduledTask -TaskPath '\GameModeExecutor\' |
    Select-Object TaskName,
                  @{n = 'RunLevel'; e = { $_.Principal.RunLevel } },
                  @{n = 'TimeLimit'; e = { $_.Settings.ExecutionTimeLimit } },
                  @{n = 'Instances'; e = { $_.Settings.MultipleInstancesPolicy } } |
    Format-Table -AutoSize

Write-Host "Try them now, from a NORMAL (non-elevated) window:" -ForegroundColor Cyan
foreach ($profile in $Profiles) {
    Write-Host "   schtasks /Run /TN `"GameModeExecutor\FanControl $profile`""
}
Write-Host "FanControl's active configuration should change each time."
