# Registers the two scheduled tasks this recipe needs, filling in the
# placeholders for you.
#
# Run it from any PowerShell window: it elevates itself, with one prompt.
# Refuse the prompt and nothing is changed.
#
# FanControl requires administrator rights because it talks to hardware, and
# GameModeExecutor runs without them on purpose, so it cannot start FanControl
# directly. The bridge is one scheduled task per role, registered once with
# "run with highest privileges". Triggering one afterwards needs no rights and
# raises no prompt.
#
# Two roles, with fixed names, so config.toml is the same for everyone:
#
#   \GameModeExecutor\FanControl Idle   applied when no game is running
#   \GameModeExecutor\FanControl Game   applied while a game is running
#
# Which FanControl configuration each role applies is yours -- FanControl's
# word for a saved set of fan curves, the files in its Configurations folder.
# It is asked for, or given with -IdleConfiguration / -GameConfiguration, and
# it lives in the task's argument. Call yours whatever you like.
#
# Usage:
#   .\install-tasks.ps1                                                  asks
#   .\install-tasks.ps1 -IdleConfiguration Quiet -GameConfiguration Game  no questions
#   .\install-tasks.ps1 -FanControlDir "D:\Tools\FanControl"             if detection fails

param(
    [string] $FanControlDir,
    [string] $IdleConfiguration,
    [string] $GameConfiguration,
    # Set by elevate.ps1 when it had to open a second window for this script.
    [switch] $InNewWindow
)

$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path

# --- 1. Elevate first, or stop here with nothing changed --------------------
. (Join-Path $here 'elevate.ps1')
Assert-Elevated -ScriptPath $PSCommandPath -BoundParameters $PSBoundParameters

# --- 2. Find FanControl -----------------------------------------------------
# The installer puts it under Program Files (x86), possibly as a service --
# which changes nothing here, -c reaches it the same way. The archive goes
# wherever it was extracted.
if (-not $FanControlDir) {
    $found = @(
        (Get-Process FanControl -ErrorAction SilentlyContinue | Select-Object -First 1).Path,
        "${env:ProgramFiles(x86)}\FanControl\FanControl.exe",
        "$env:ProgramFiles\FanControl\FanControl.exe",
        "$env:LOCALAPPDATA\Programs\FanControl\FanControl.exe",
        "$env:USERPROFILE\scoop\apps\fancontrol\current\FanControl.exe"
    ) | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1

    if ($found) { $FanControlDir = Split-Path $found }
}

if (-not $FanControlDir -or -not (Test-Path (Join-Path $FanControlDir 'FanControl.exe'))) {
    Write-Host "FanControl.exe not found." -ForegroundColor Red
    Write-Host "Run again naming its folder, for example:"
    Write-Host '   .\install-tasks.ps1 -FanControlDir "D:\Tools\FanControl"'
    Write-Host "(right-click your FanControl shortcut -> Open file location)"
    Leave 1
}
Write-Host "FanControl     : $FanControlDir" -ForegroundColor Cyan

# --- 3. Which configuration plays which role ---------------------------------
# Configurations cannot be shipped: a fan curve depends on the machine's
# hardware, so someone else's would be useless at best. What can be done is
# list the ones you saved, and ask.
$folder = Join-Path $FanControlDir 'Configurations'
$available = @(Get-ChildItem $folder -Filter *.json -ErrorAction SilentlyContinue |
               ForEach-Object { $_.BaseName })
if ($available) {
    Write-Host "Configurations : $($available -join ', ')" -ForegroundColor Cyan
} else {
    Write-Host "Configurations : none saved yet in $folder" -ForegroundColor Yellow
}

# FanControl notes which configuration it is applying in Configurations\CACHE
# -- JSON, "CurrentConfigFileName": "Quiet.json", as observed on 2026-09-17.
# Nobody runs this script in the middle of a game, so the one active now is
# almost always the everyday one: the natural default for Idle, and no
# default at all for Game.
$active = $null
$cache = Join-Path $folder 'CACHE'
if (Test-Path $cache) {
    try {
        $active = (Get-Content $cache -Raw | ConvertFrom-Json).CurrentConfigFileName -replace '\.json$', ''
    } catch {
        $active = $null
    }
}
if ($active) {
    Write-Host "Active now     : $active" -ForegroundColor Cyan
}

function Choose-Configuration([string] $Role, [string] $Meaning, [string] $Given, [string] $Fallback) {
    $name = $Given
    if (-not $name) {
        # One named after the role is the obvious default, then whatever the
        # caller suggests; anything else is a question, not a guess.
        $default = $available | Where-Object { $_ -ieq $Role } | Select-Object -First 1
        if (-not $default -and $Fallback) {
            $default = $available | Where-Object { $_ -ieq $Fallback } | Select-Object -First 1
        }
        $hint = if ($default) { " [$default]" } else { '' }
        Write-Host ""
        Write-Host "Configuration for $Role -- $Meaning" -ForegroundColor Cyan
        $name = Read-Host "  name$hint"
        if (-not $name) { $name = $default }
        if (-not $name) {
            Write-Host "No configuration named for $Role. Nothing was changed." -ForegroundColor Red
            Write-Host "Run again with -${Role}Configuration <name>, or answer the question."
            Leave 1
        }
    }
    $name = $name.Trim() -replace '\.json$', ''
    # Use the casing on disk: FanControl may compare the name exactly.
    $onDisk = $available | Where-Object { $_ -ieq $name } | Select-Object -First 1
    if ($onDisk) {
        return $onDisk
    }
    Write-Host "  '$name.json' is not in $folder yet." -ForegroundColor Yellow
    Write-Host "  Registered anyway; the task does nothing until that configuration is saved in FanControl."
    return $name
}

$IdleConfiguration = Choose-Configuration -Role 'Idle' -Meaning 'applied when no game is running' -Given $IdleConfiguration -Fallback $active
$GameConfiguration = Choose-Configuration -Role 'Game' -Meaning 'applied while a game is running' -Given $GameConfiguration

Write-Host ""
Write-Host "Idle           : $IdleConfiguration.json" -ForegroundColor Cyan
Write-Host "Game           : $GameConfiguration.json" -ForegroundColor Cyan

# --- 4. Register ------------------------------------------------------------
$user = "$env:USERDOMAIN\$env:USERNAME"
Write-Host "Account        : $user" -ForegroundColor Cyan
Write-Host ""

$roles = @(
    @{ Role = 'Idle'; Configuration = $IdleConfiguration },
    @{ Role = 'Game'; Configuration = $GameConfiguration }
)
foreach ($entry in $roles) {
    $role = $entry.Role
    $template = Join-Path $here "FanControl-$role.xml"
    if (-not (Test-Path $template)) {
        Write-Host "  template FanControl-$role.xml is missing next to this script" -ForegroundColor Red
        Leave 1
    }

    $xml = [System.IO.File]::ReadAllText($template, [System.Text.Encoding]::Unicode)
    $xml = $xml.Replace('__FANCONTROL_DIR__', $FanControlDir)
    $xml = $xml.Replace('__DOMAIN__\__USERNAME__', $user)
    $xml = $xml.Replace('__CONFIGURATION__', $entry.Configuration)

    Register-ScheduledTask -Xml $xml -TaskName "FanControl $role" `
                           -TaskPath '\GameModeExecutor\' -Force | Out-Null
    Write-Host "  registered \GameModeExecutor\FanControl $role  ->  -c $($entry.Configuration).json" -ForegroundColor Green
}

# --- 5. Show what landed ----------------------------------------------------
Write-Host ""
Write-Host "In \GameModeExecutor :" -ForegroundColor Cyan
Get-ScheduledTask -TaskPath '\GameModeExecutor\' |
    Select-Object TaskName,
                  @{n = 'Applies';   e = { $_.Actions[0].Arguments } },
                  @{n = 'RunLevel';  e = { $_.Principal.RunLevel } },
                  @{n = 'TimeLimit'; e = { $_.Settings.ExecutionTimeLimit } },
                  @{n = 'Instances'; e = { $_.Settings.MultipleInstances } } |
    Format-Table -AutoSize

# An earlier version of this recipe named the tasks after the configurations
# ("FanControl Quiet"). Those still work, but nothing points at them any more.
$managed = 'FanControl Idle', 'FanControl Game'
$others = @(Get-ScheduledTask -TaskPath '\GameModeExecutor\' |
            Where-Object { $_.TaskName -like 'FanControl *' -and $_.TaskName -notin $managed })
if ($others) {
    Write-Host "Also there, not managed by this script:" -ForegroundColor Yellow
    foreach ($task in $others) {
        Write-Host "   $($task.TaskName)  ($($task.Actions[0].Arguments))"
    }
    Write-Host "If these are leftovers from an earlier version of this recipe, remove them:"
    foreach ($task in $others) {
        Write-Host "   Unregister-ScheduledTask -TaskPath '\GameModeExecutor\' -TaskName '$($task.TaskName)' -Confirm:`$false"
    }
    Write-Host ""
}

Write-Host "Try them now, from a NORMAL (non-elevated) window:" -ForegroundColor Cyan
Write-Host '   schtasks /Run /TN "GameModeExecutor\FanControl Game"'
Write-Host '   schtasks /Run /TN "GameModeExecutor\FanControl Idle"'
Write-Host "FanControl's active configuration should change each time."
Leave
