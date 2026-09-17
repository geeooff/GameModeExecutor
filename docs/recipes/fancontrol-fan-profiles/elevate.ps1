# Relaunches the calling script elevated when it is not, so it can be run
# from any window. Shared by install-tasks.ps1 and uninstall-tasks.ps1.
#
# Two ways up, measured on 2026-09-17 on Windows 11 24H2:
#
#   - Windows' own `sudo`, when it is enabled in *inline* mode: the elevated
#     script runs in this same window, questions and output included, and its
#     exit code comes back. In its other modes sudo opens a new window and
#     returns at once with exit code 0 whatever happened, which is useless
#     here, so those count as "no sudo".
#   - Otherwise the ordinary "run as administrator" prompt: a second window
#     opens, this one waits for it, and the exit code comes back. That window
#     would close the moment the script ends, so the script is told
#     -InNewWindow and waits for Enter before closing.
#
# Refusing the prompt is not an error of the script: sudo answers 0x800704C7
# and Start-Process throws "canceled by the user". Both end here with exit
# code 1 and nothing changed.

function Assert-Elevated {
    param(
        [Parameter(Mandatory)] [string] $ScriptPath,
        [System.Collections.IDictionary] $BoundParameters = @{},
        # 'auto' takes sudo when it is inline and the prompt otherwise. The
        # other two force one path, which is how both get tested on one
        # machine.
        [ValidateSet('auto', 'sudo', 'runas')] [string] $Mechanism = 'auto'
    )

    $admin = ([Security.Principal.WindowsPrincipal] [Security.Principal.WindowsIdentity]::GetCurrent()
             ).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
    if ($admin) {
        return
    }

    # The same host as the caller -- pwsh or Windows PowerShell -- and the
    # caller's own arguments, forwarded as given.
    $shell = (Get-Process -Id $PID).Path
    $arguments = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $ScriptPath)
    foreach ($entry in $BoundParameters.GetEnumerator()) {
        if ($entry.Value -is [switch]) {
            if ($entry.Value.IsPresent) { $arguments += "-$($entry.Key)" }
        } else {
            $arguments += "-$($entry.Key)", [string] $entry.Value
        }
    }

    # Inline sudo: Enabled = 3 under the machine key, or under the policy key
    # when an administrator has set one. 1 and 2 are the new-window modes.
    $mode = 0
    foreach ($key in 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\Sudo',
                     'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Sudo') {
        $value = (Get-ItemProperty -Path $key -Name Enabled -ErrorAction SilentlyContinue).Enabled
        if ($null -ne $value) { $mode = $value; break }
    }
    $sudo = Get-Command sudo -ErrorAction SilentlyContinue
    $useSudo = switch ($Mechanism) {
        'sudo'  { [bool] $sudo }
        'runas' { $false }
        default { $sudo -and $mode -eq 3 }
    }

    if ($useSudo) {
        # A native call: PowerShell quotes each argument as needed, so none
        # are quoted here.
        Write-Host "Elevating in this window through sudo (accept the prompt)..." -ForegroundColor Cyan
        & $sudo.Source $shell @arguments
        $code = $LASTEXITCODE
        if ($code -eq -2147023673) {
            # 0x800704C7, ERROR_CANCELLED
            Write-Host "Elevation was refused. Nothing was changed." -ForegroundColor Yellow
            exit 1
        }
        exit $code
    }

    # Start-Process joins the list into one command line itself, so anything
    # with a space has to be quoted here.
    $commandLine = ($arguments + '-InNewWindow') | ForEach-Object {
        if ($_ -match '\s') { "`"$_`"" } else { $_ }
    }
    Write-Host "Elevating in a new window (accept the prompt; that window waits for Enter)..." -ForegroundColor Cyan
    try {
        $child = Start-Process $shell -Verb RunAs -Wait -PassThru -ArgumentList $commandLine
    } catch {
        Write-Host "Elevation was refused. Nothing was changed." -ForegroundColor Yellow
        exit 1
    }
    exit $child.ExitCode
}

# Every way out of the calling script goes through here, so a window that was
# opened for the elevated copy does not vanish with the result still in it.
# Inline sudo and an already elevated window need no pause. Reads the caller's
# own $InNewWindow, which dot-sourcing puts in scope.
function Leave([int] $Code = 0) {
    if ($InNewWindow) {
        Write-Host ""
        Read-Host "Press Enter to close this window" | Out-Null
    }
    exit $Code
}
