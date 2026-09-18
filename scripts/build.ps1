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

# Runs the Windows SDK's Internal Consistency Evaluators over a package.
#
# MsiVal2 ships in the SDK as an installer of its own; an administrative
# install (msiexec /a) unpacks it into target\ without elevation. Its ICE
# evaluator, evalcom2.dll, is a COM server the tool creates by ProgID, and
# the SDK's own package registers it under the CLSID of the *old* evalcom.dll,
# which the new one refuses -- measured 2026-09-17; Orca's package has the
# right one. So the CLSID is registered here, for this user only, under
# HKCU\Software\Classes, which needs no elevation. Returns the findings
# (errors and warnings) and how many evaluators ran.
function Invoke-Ice([string] $Package) {
    $kits = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
    $source = Get-ChildItem (Join-Path $kits '10.0.*\x86\MsiVal2-x86_en-us.msi') -ErrorAction SilentlyContinue |
              Sort-Object FullName | Select-Object -Last 1
    if (-not $source) { Fail "the Windows SDK's MsiVal2 package is not under $kits" }

    $val2 = Join-Path $root 'target\msival2'
    $tool = Join-Path $val2 'MsiVal2\MsiVal2.exe'
    if (-not (Test-Path $tool)) {
        $extract = Start-Process msiexec.exe -Wait -PassThru `
                   -ArgumentList @('/a', "`"$($source.FullName)`"", '/qn', "TARGETDIR=`"$val2`"")
        if ($extract.ExitCode -ne 0 -or -not (Test-Path $tool)) { Fail "cannot unpack MsiVal2 (msiexec /a exited $($extract.ExitCode))" }
    }

    $dll = Join-Path $val2 'MsiVal2\evalcom2.dll'
    $clsid = '{6E5E1910-8053-4660-B795-6B612E29BC58}'
    foreach ($classes in 'HKCU:\Software\Classes', 'HKCU:\Software\Classes\WOW6432Node') {
        $server = "$classes\CLSID\$clsid\InProcServer32"
        if ((Get-ItemProperty $server -ErrorAction SilentlyContinue).'(default)' -ne $dll) {
            New-Item -Path $server -Force | Out-Null
            New-Item -Path "$classes\CLSID\$clsid\ProgID" -Force | Out-Null
            New-Item -Path "$classes\MSI.EVALCOM2.1\CLSID" -Force | Out-Null
            Set-ItemProperty -Path $server -Name '(default)' -Value $dll
            Set-ItemProperty -Path $server -Name 'ThreadingModel' -Value 'Apartment'
            Set-ItemProperty -Path "$classes\CLSID\$clsid\ProgID" -Name '(default)' -Value 'MSI.EVALCOM2.1'
            Set-ItemProperty -Path "$classes\MSI.EVALCOM2.1\CLSID" -Name '(default)' -Value $clsid
        }
    }

    $output = & $tool $Package (Join-Path $val2 'MsiVal2\darice.cub') 2>&1 | ForEach-Object { "$_" }
    if ($output -match 'Fatal Error') { Fail "MsiVal2 could not run: $($output -join ' ')" }
    $ran = @($output | ForEach-Object { if ($_ -match '^(ICE\d+)\s') { $Matches[1] } } | Sort-Object -Unique).Count
    $findings = @($output | Where-Object { $_ -match '^ICE\d+\s+(ERROR|WARNING)' } | ForEach-Object { $_.Trim() })
    [pscustomobject] @{ Ran = $ran; Findings = $findings }
}

# --- the checks -------------------------------------------------------------

function Invoke-Tests {
    Step "Formatting"
    Run 'cargo' @('fmt', '--check')
    Write-Host "    clean"

    Step "Clippy, warnings are errors"
    Run 'cargo' @('clippy', '--all-targets', '--', '-D', 'warnings')

    # Doc comments are code too: a link to a private item or a stray [bracket]
    # is a warning rustdoc would print to whoever reads the API.
    Step "Doc comments build without warnings"
    $env:RUSTDOCFLAGS = '-D warnings'
    try { Run 'cargo' @('doc', '--no-deps', '--quiet') } finally { Remove-Item Env:\RUSTDOCFLAGS -ErrorAction SilentlyContinue }

    Step "Tests"
    Run 'cargo' @('test')

    # A few tests read this machine's registry -- the Known Game List, the
    # Game Bar registration -- which a hosted Windows Server runner does not
    # have. They are marked #[ignore] for that reason and run here, where the
    # machine is a real Windows client. GitHub sets CI=true on its runners.
    if (-not $env:CI) {
        Step "Tests that need a Windows client"
        # One ignored test is left out on purpose: it starts Windows' presence
        # writer for real, and an installed watcher on this machine would run
        # the user's own commands in reaction. It is run by name when wanted.
        Run 'cargo' @('test', '--', '--ignored', '--skip', 'a_real_activation_drives_a_session')
    }

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
    $pages = @((Join-Path $root 'README.md'), (Join-Path $root 'AGENTS.md')) +
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
    # The commit only. A `-dirty` suffix is honest and expected from a working
    # tree, and `release` refuses one separately -- comparing the whole string
    # would fail every ordinary `build`.
    $reported = ((& (Join-Path $root 'target\release\gamemode-executor.exe') --version |
                  Select-String '^commit:\s+(\S+)').Matches[0].Groups[1].Value) -replace '-dirty$', ''
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

    # build.rs writes a version block into each executable: the Properties
    # dialog reads it, and Windows Installer's repair and upgrade rules need
    # versioned files. A miss is only a warning at build time, so it is
    # checked here instead.
    Step "Version resources"
    $version = Get-Version
    foreach ($name in 'gamemode-executor.exe', 'gamemode-executorw.exe') {
        $info = (Get-Item (Join-Path $root "target\release\$name")).VersionInfo
        if ($info.ProductVersion -ne $version) { Fail "$name carries product version '$($info.ProductVersion)', expected $version" }
        if ($info.FileVersionRaw -ne [version] "$version.0") { Fail "$name carries file version $($info.FileVersionRaw), expected $version.0" }
        if ($info.OriginalFilename -ne $name) { Fail "$name says its original name is '$($info.OriginalFilename)'" }
        if (-not $info.FileDescription) { Fail "$name has no file description" }
        $flag = if ($info.IsPrivateBuild) { '  (private build: uncommitted changes)' } else { '' }
        Write-Host ("    {0,-24} {1}{2}" -f $name, $info.FileVersion, $flag)
    }

    # The release profile -- opt-level z, LTO, strip, panic = abort -- keeps
    # each binary near 1.1 MB. Losing it is silent: everything still builds and
    # runs, only twice as large and unwinding on panic, which the FATAL hook was
    # not designed for. It happened once, from an editing slip in Cargo.toml.
    Step "Binaries are release-profile sized"
    $ceiling = 1.75MB
    foreach ($name in $expected.Keys | Sort-Object) {
        $size = (Get-Item (Join-Path $root "target\release\$name")).Length
        if ($size -gt $ceiling) {
            Fail "$name is $([math]::Round($size / 1MB, 2)) MB; check [profile.release] in Cargo.toml"
        }
        Write-Host ("    {0,-24} {1,6:N0} KB" -f $name, ($size / 1KB))
    }
}

function Invoke-Release {
    $version = Get-Version
    $stage = Join-Path $root "dist\GameModeExecutor-$version"
    $zip = Join-Path $root "dist\GameModeExecutor-$version.zip"

    Step "Staging $version"
    if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
    New-Item -ItemType Directory -Force -Path $stage | Out-Null

    Copy-Item (Join-Path $root 'target\release\gamemode-executor.exe')  $stage
    Copy-Item (Join-Path $root 'target\release\gamemode-executorw.exe') $stage
    Copy-Item (Join-Path $root 'LICENSE') $stage
    # No configuration in the zip: `init` writes the starter one where the
    # installer would, so both ways in leave the same machine behind.
    # The docs ship as they are, rather than being rewritten for the bundle.
    # One copy means the bundle cannot describe a version that no longer exists.
    Copy-Item (Join-Path $root 'docs') $stage -Recurse

    # Asked of the binary rather than of git, so the readme cannot claim a
    # commit different from the one actually compiled in.
    $stamp = & (Join-Path $stage 'gamemode-executor.exe') --version

    Set-Content -Path (Join-Path $stage 'README.txt') -Encoding UTF8 -Value @"
GameModeExecutor $version - zip archive

$($stamp -join "`r`n")

The documentation link above names the exact commit these executables were
built from, so it describes this build and not whatever the project looks like
by the time you follow it.


Runs the programs you configure when a game starts, and others when it stops.
There is no list of games to maintain: detection is Windows' own.

Nothing to install. Keep this folder where you put it -- the scheduled task
will remember this path. Then, from a terminal in this folder:

    gamemode-executor init            writes a starter configuration
    gamemode-executor install-task    starts the watcher now and at every logon

The starter configuration runs nothing; the icon that appears shows the
watcher is working. What to run is yours to write -- docs\recipes\ has
worked examples, one folder each.

START HERE
    docs\getting-started.md, next to this file -- or the documentation link
    above, which is the same page at the exact commit this was built from.

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

    # The installer: the same two executables and the license, per-user, no
    # elevation, built by scripts\msi.ps1 from Windows Installer's own
    # automation. The commit and the documentation link come from the
    # binary, as the readme's do.
    Step "Windows Installer package"
    $msi = Join-Path $root "dist\GameModeExecutor-$version.msi"
    $docLink = ($stamp | Select-String -Pattern '^documentation:\s+(\S+)').Matches[0].Groups[1].Value
    $commit = ($stamp | Select-String -Pattern '^commit:\s+([0-9a-f]{8})').Matches[0].Groups[1].Value
    $package = & (Join-Path $root 'scripts\msi.ps1') -Stage $stage -Version $version -Out $msi `
                 -Commit $commit -DocumentationUrl $docLink `
                 -Icon (Join-Path $root 'assets\icons\gamemode-active-light.ico')
    Write-Host "    product $($package.ProductCode)"
    Write-Host "    package $($package.PackageCode)"

    # Every ICE the SDK ships, and nothing tolerated: a warning here is a
    # package that behaves oddly on someone else's machine.
    Step "Package validation"
    $ice = Invoke-Ice -Package $msi
    if ($ice.Findings) {
        $ice.Findings | ForEach-Object { Write-Host "    $_" -ForegroundColor Red }
        Fail "the package has ICE findings"
    }
    Write-Host "    $($ice.Ran) evaluators ran, no errors, no warnings"

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
        foreach ($placeholder in '__FANCONTROL_DIR__', '__DOMAIN__\__USERNAME__', '__CONFIGURATION__') {
            if ($content -notlike "*$placeholder*") {
                Fail "$($template.Name) has lost its $placeholder placeholder"
            }
        }
    }
    Write-Host "    clean, and the task templates still have their placeholders"

    Write-Host ""
    Write-Host "dist\GameModeExecutor-$version.zip  ($([math]::Round((Get-Item $zip).Length / 1KB)) KB)" -ForegroundColor Green
    Write-Host "dist\GameModeExecutor-$version.msi  ($([math]::Round((Get-Item $msi).Length / 1KB)) KB)" -ForegroundColor Green
}

# --- go ---------------------------------------------------------------------

if ($Task -eq 'release') { Assert-CleanTree }
Invoke-Tests
if ($Task -in @('build', 'release')) { Invoke-Build }
if ($Task -eq 'release') { Invoke-Release }

Write-Host ""
Write-Host "$Task OK" -ForegroundColor Green
