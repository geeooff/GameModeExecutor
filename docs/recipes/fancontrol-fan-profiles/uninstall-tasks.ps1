# Removes the two scheduled tasks install-tasks.ps1 registered, and nothing
# else.
#
# Run it from any PowerShell window: it elevates itself, with one prompt.
# Refuse the prompt and nothing is changed.
#
# What goes:
#
#   \GameModeExecutor\FanControl Idle
#   \GameModeExecutor\FanControl Game
#
# What stays: FanControl and its configurations, the \GameModeExecutor folder
# in Task Scheduler and every other task in it -- the watcher's own logon task
# lives there -- and GameModeExecutor's config.toml, which still names the two
# tasks until you edit it. This script is the recipe's counterpart to
# install-tasks.ps1; GameModeExecutor itself does not know which recipe you
# followed and never removes a task it did not register.
#
# Usage:
#   .\uninstall-tasks.ps1

param(
    # Set by elevate.ps1 when it had to open a second window for this script.
    [switch] $InNewWindow
)

$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path

# --- 1. Elevate first, or stop here with nothing changed --------------------
. (Join-Path $here 'elevate.ps1')
Assert-Elevated -ScriptPath $PSCommandPath -BoundParameters $PSBoundParameters

# --- 2. Remove the two roles ------------------------------------------------
$managed = 'FanControl Idle', 'FanControl Game'
$removed = 0
foreach ($name in $managed) {
    $task = Get-ScheduledTask -TaskPath '\GameModeExecutor\' -TaskName $name -ErrorAction SilentlyContinue
    if ($task) {
        Unregister-ScheduledTask -TaskPath '\GameModeExecutor\' -TaskName $name -Confirm:$false
        Write-Host "  removed \GameModeExecutor\$name  (was $($task.Actions[0].Arguments))" -ForegroundColor Green
        $removed++
    } else {
        Write-Host "  \GameModeExecutor\$name was not there"
    }
}
if ($removed -eq 0) {
    Write-Host "Nothing to remove; nothing was changed."
}

# --- 3. Say what was left alone ---------------------------------------------
# An earlier version of this recipe named the tasks after the configurations
# ("FanControl Quiet"). Not registered by this script, so not removed by it.
$others = @(Get-ScheduledTask -TaskPath '\GameModeExecutor\' -ErrorAction SilentlyContinue |
            Where-Object { $_.TaskName -like 'FanControl *' })
if ($others) {
    Write-Host ""
    Write-Host "Also in \GameModeExecutor, left alone:" -ForegroundColor Yellow
    foreach ($task in $others) {
        Write-Host "   $($task.TaskName)  ($($task.Actions[0].Arguments))"
    }
    Write-Host "If these are leftovers from an earlier version of this recipe, remove them:"
    foreach ($task in $others) {
        Write-Host "   Unregister-ScheduledTask -TaskPath '\GameModeExecutor\' -TaskName '$($task.TaskName)' -Confirm:`$false"
    }
}

# --- 4. What this does not do -----------------------------------------------
Write-Host ""
Write-Host "FanControl keeps whichever configuration is active right now; pick the one" -ForegroundColor Cyan
Write-Host "you want in FanControl itself." -ForegroundColor Cyan
Write-Host "GameModeExecutor's config.toml still names these tasks: edit it, or the" -ForegroundColor Cyan
Write-Host "watcher will report a failed command at the next game." -ForegroundColor Cyan
Leave
