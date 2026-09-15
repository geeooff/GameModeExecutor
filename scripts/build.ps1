# Test, build and package GameModeExecutor.
#
# This is the checklist that was being run by hand, which is why it exists: run
# by hand it was skipped twice -- once committing a failing test, once shipping
# a scheduled task with a relative path in it. A checklist nobody forgets is a
# script.
#
#   .\scripts\build.ps1            # test (the default)
#   .\scripts\build.ps1 build      # test, then build release
#   .\scripts\build.ps1 release    # test, build, and zip into dist\
#
# From VS Code: Terminal -> Run Task, or Ctrl+Shift+B for the build.

param(
    [ValidateSet('test', 'build', 'release')]
    [string] $Task = 'test'
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root

# --- plumbing ---------------------------------------------------------------

$script:step = 0

function Step([string] $title) {
    $script:step++
    Write-Host ""
    Write-Host "[$script:step] $title" -ForegroundColor Cyan
}

function Fail([string] $message) {
    Write-Host ""
    Write-Host "FAILED: $message" -ForegroundColor Red
    exit 1
}

# Runs a native command and stops the script if it returns non-zero.
# PowerShell does not do this on its own, which is how a red test run gets
# committed.
function Run([string] $command, [string[]] $commandArgs) {
    & $command @commandArgs
    if ($LASTEXITCODE -ne 0) { Fail "$command $($commandArgs -join ' ') exited $LASTEXITCODE" }
}

# The binaries carry the commit they were built from, and a release is the one
# artefact where that claim has to be true: from a dirty tree it would name a
# commit that does not contain what was built, and the documentation link would
# point at code the user does not have. Checked before anything else, so a
# dirty tree costs a second rather than a full build.
function Assert-CleanTree {
    Step "Working tree is clean"
    $changes = & git status --porcelain
    if ($LASTEXITCODE -ne 0) {
        Fail "git is unavailable here, so the commit cannot be stamped into a release"
    }
    if ($changes) {
        $changes | Select-Object -First 10 | ForEach-Object { Write-Host "    $_" -ForegroundColor Red }
        if (@($changes).Count -gt 10) { Write-Host "    ... and more" -ForegroundColor Red }
        Fail "commit or stash first -- a release must name a commit that contains what it ships"
    }
    Write-Host "    $(& git rev-parse --short HEAD)"
}

function Get-Version {
    $line = Select-String -Path (Join-Path $root 'Cargo.toml') -Pattern '^version\s*=\s*"([^"]+)"' |
            Select-Object -First 1
    if (-not $line) { Fail "cannot read the version from Cargo.toml" }
    $line.Matches[0].Groups[1].Value
}

# --- the checks -------------------------------------------------------------

function Invoke-Tests {
    Step "Formatting"
    Run 'cargo' @('fmt', '--check')
    Write-Host "    clean"

    Step "Clippy, warnings are errors"
    Run 'cargo' @('clippy', '--all-targets', '--', '-D', 'warnings')

    Step "Tests"
    Run 'cargo' @('test')

    Step "Shipped configurations parse"
    # These are files people copy over their own, so a typo in one is a typo in
    # theirs. Checked with the program itself rather than by eye.
    Run 'cargo' @('build', '--quiet')
    $exe = Join-Path $root 'target\debug\gamemode-executor.exe'
    $configs = @(Join-Path $root 'config.example.toml') +
               (Get-ChildItem (Join-Path $root 'docs\recipes') -Recurse -Filter 'config.toml' |
                    ForEach-Object { $_.FullName })
    foreach ($config in $configs) {
        $relative = $config.Substring($root.Length + 1)
        & $exe --config $config validate | Out-Null
        if ($LASTEXITCODE -ne 0) {
            & $exe --config $config validate
            Fail "$relative is not valid"
        }
        Write-Host "    $relative"
    }

    Step "Documentation links resolve"
    $broken = @()
    $pages = @(Join-Path $root 'README.md') +
             (Get-ChildItem (Join-Path $root 'docs') -Recurse -Filter '*.md' |
                  ForEach-Object { $_.FullName })
    foreach ($page in $pages) {
        $dir = Split-Path -Parent $page
        foreach ($match in [regex]::Matches((Get-Content $page -Raw), '\]\(([^)]+)\)')) {
            $link = $match.Groups[1].Value
            if ($link -match '^(https?:|#)') { continue }
            $target = ($link -split '#')[0]
            if (-not $target) { continue }
            if (-not (Test-Path (Join-Path $dir $target))) {
                $broken += "$($page.Substring($root.Length + 1)) -> $link"
            }
        }
    }
    if ($broken) {
        $broken | ForEach-Object { Write-Host "    broken: $_" -ForegroundColor Red }
        Fail "$($broken.Count) broken documentation link(s)"
    }
    Write-Host "    $($pages.Count) pages, every link resolves"
}

function Invoke-Build {
    Step "Release build"
    Run 'cargo' @('build', '--release')

    # A console program and a windowless one cannot be the same file, and
    # getting that wrong is invisible until someone sees a black window at
    # logon. Read it out of the PE header rather than trusting the setting.
    # The binaries claim a commit, and a release is where that claim has to be
    # true. A stale stamp is silent: the executable runs perfectly and points
    # its documentation link at code the user does not have. One release shipped
    # that way before this check existed, because build.rs was watching
    # .git/HEAD, which does not change when you commit on a branch.
    Step "The stamped commit is this commit"
    $expected = & git rev-parse HEAD
    $reported = (& (Join-Path $root 'target\release\gamemode-executor.exe') --version |
                 Select-String '^commit:\s+(\S+)').Matches[0].Groups[1].Value
    if ($reported -ne $expected) {
        Write-Host "    built binary says $reported" -ForegroundColor Red
        Write-Host "    HEAD is          $expected" -ForegroundColor Red
        Fail "the stamp is stale -- `cargo clean` and build again"
    }
    Write-Host "    $reported"

    Step "Subsystems"
    $expected = @{ 'gamemode-executor.exe' = 3; 'gamemode-executorw.exe' = 2 }
    $label = @{ 2 = 'WINDOWS_GUI (no console)'; 3 = 'WINDOWS_CUI (console)' }
    foreach ($name in $expected.Keys | Sort-Object) {
        $path = Join-Path $root "target\release\$name"
        if (-not (Test-Path $path)) { Fail "$name was not built" }
        $bytes = [System.IO.File]::ReadAllBytes($path)
        $peOffset = [BitConverter]::ToInt32($bytes, 0x3C)
        # [int], because the hashtable lookup below would otherwise miss: a
        # UInt16 3 and an Int32 3 are not the same key to PowerShell.
        $subsystem = [int][BitConverter]::ToUInt16($bytes, $peOffset + 24 + 0x44)
        if ($subsystem -ne $expected[$name]) {
            Fail "$name is subsystem $subsystem, expected $($expected[$name])"
        }
        Write-Host ("    {0,-24} {1}" -f $name, $label[$subsystem])
    }
}

function Invoke-Release {
    $version = Get-Version
    $stage = Join-Path $root "dist\GameModeExecutor-$version"
    $zip = Join-Path $root "dist\GameModeExecutor-$version-portable.zip"

    Step "Staging $version"
    if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
    New-Item -ItemType Directory -Force -Path $stage | Out-Null

    Copy-Item (Join-Path $root 'target\release\gamemode-executor.exe')  $stage
    Copy-Item (Join-Path $root 'target\release\gamemode-executorw.exe') $stage
    Copy-Item (Join-Path $root 'LICENSE') $stage
    # The FanControl recipe's configuration is the shipped default: it is the
    # case this program was built for, and it is inert until its tasks exist.
    Copy-Item (Join-Path $root 'docs\recipes\fancontrol-fan-profiles\config.toml') `
              (Join-Path $stage 'config.toml')
    # The docs ship as they are, rather than being rewritten for the bundle.
    # One copy means the bundle cannot describe a version that no longer exists.
    Copy-Item (Join-Path $root 'docs') $stage -Recurse

    # Asked of the binary rather than of git, so the readme cannot claim a
    # commit different from the one actually compiled in.
    $stamp = & (Join-Path $stage 'gamemode-executor.exe') --version

    Set-Content -Path (Join-Path $stage 'README.txt') -Encoding UTF8 -Value @"
GameModeExecutor $version - portable

$($stamp -join "`r`n")

The documentation link above names the exact commit these executables were
built from, so it describes this build and not whatever the project looks like
by the time you follow it.


Runs the programs you configure when a game starts, and others when it stops.
There is no list of games to maintain: detection is Windows' own.

Nothing to install. Keep this folder where you put it -- the scheduled task
will remember this path.

START HERE
    docs\getting-started.md, next to this file -- or the documentation link
    above, which is the same page at the exact commit this was built from.

THE RECIPE THIS BUNDLE IS SET UP FOR
    docs\recipes\fancontrol-fan-profiles\
    Quiet fans outside games, a game profile while playing, with FanControl.
    config.toml here is already that recipe. It does nothing until you
    register its two scheduled tasks -- that folder has a script for it.

    You must create the two FanControl profiles yourself, named Quiet.json and
    Game.json. They are not shipped and cannot be: a fan curve depends on the
    machine's own hardware.

THE TWO EXECUTABLES
    gamemode-executor.exe    the one you talk to. Every command. It answers,
                             then it is done.
    gamemode-executorw.exe   the one that works. No window, ever. It starts
                             itself at logon. You never launch it yourself.

QUICK CHECK
    .\gamemode-executor.exe validate
    .\gamemode-executor.exe status
    .\gamemode-executor.exe install-task     (start it at every logon)

MIT licensed. Full documentation and source:
https://github.com/Geeooff/GameModeExecutor
"@

    Step "Packaging"
    if (Test-Path $zip) { Remove-Item $zip }
    Compress-Archive -Path $stage -DestinationPath $zip -CompressionLevel Optimal

    # A personal path baked into a public artefact is the kind of thing nobody
    # looks for until it is already published.
    Step "Nothing local leaked"
    $text = Get-ChildItem $stage -Recurse -File -Include *.toml, *.xml, *.ps1, *.txt, *.md
    $leaks = $text | Select-String -Pattern ([regex]::Escape($env:USERNAME)),
                                            ([regex]::Escape($env:USERDOMAIN)) -List
    if ($leaks) {
        $leaks | ForEach-Object { Write-Host "    $($_.Filename): $($_.Line.Trim())" -ForegroundColor Red }
        Fail "the bundle names this machine's account"
    }

    # The opposite mistake, and the likelier one: a template whose placeholders
    # were filled in on the way past, so it carries one machine's paths to
    # every other.
    foreach ($template in Get-ChildItem (Join-Path $stage 'docs') -Recurse -Filter 'FanControl-*.xml') {
        $content = [System.IO.File]::ReadAllText($template.FullName, [System.Text.Encoding]::Unicode)
        foreach ($placeholder in '__FANCONTROL_DIR__', '__DOMAIN__\__USERNAME__') {
            if ($content -notlike "*$placeholder*") {
                Fail "$($template.Name) has lost its $placeholder placeholder"
            }
        }
    }
    Write-Host "    clean, and the task templates still have their placeholders"

    Write-Host ""
    Write-Host "dist\GameModeExecutor-$version-portable.zip  ($([math]::Round((Get-Item $zip).Length / 1KB)) KB)" -ForegroundColor Green
}

# --- go ---------------------------------------------------------------------

if ($Task -eq 'release') { Assert-CleanTree }
Invoke-Tests
if ($Task -in @('build', 'release')) { Invoke-Build }
if ($Task -eq 'release') { Invoke-Release }

Write-Host ""
Write-Host "$Task OK" -ForegroundColor Green
