# Writes the release notes for a tag: the section CHANGELOG.md carries for
# that version -- written before the release, for the person running the
# program -- then the documentation link the binary carries, the commits
# since the previous tag for the curious, and the checksums of the three
# artefacts. Run by the release workflow after scripts\build.ps1 release;
# nothing in it needs GitHub. A version without its section is refused:
# the workflow cannot summarise, and a release must have something to say.
#
#   .\scripts\release-notes.ps1 -Tag v0.1.0            writes dist\notes.md and dist\SHA256SUMS.txt
param(
    [Parameter(Mandatory)] [string] $Tag
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$dist = Join-Path $root 'dist'

if ($Tag -notmatch '^v(\d+\.\d+\.\d+)$') { throw "tag `"$Tag`" is not vX.Y.Z" }
$version = $Matches[1]

# The notes proper, from CHANGELOG.md: the section for this version,
# heading excluded, up to the next one. Refused when absent, the way a tag
# that disagrees with Cargo.toml is refused.
$changelog = Get-Content (Join-Path $root 'CHANGELOG.md') -Raw
$pattern = "(?ms)^## \[$([regex]::Escape($version))\] - \d{4}-\d{2}-\d{2}[^\r\n]*\r?\n(.*?)(?=^## |\z)"
$section = [regex]::Match($changelog, $pattern)
if (-not $section.Success) { throw "CHANGELOG.md has no dated section for $version; a release commit carries ``## [$version] - YYYY-MM-DD``" }
# The last section is followed by the link definitions Keep a Changelog
# keeps at the bottom; those are the file's, not the release's.
$notes = (($section.Groups[1].Value -replace '\r\n', "`n") -split "`n" |
          Where-Object { $_ -notmatch '^\[[^\]]+\]: ' }) -join "`n"
$notes = $notes.Trim()
$msi = Join-Path $dist "GameModeExecutor-$version.msi"
$zip = Join-Path $dist "GameModeExecutor-$version.zip"
$recipes = Join-Path $dist "GameModeExecutor-recipes-$version.zip"
foreach ($artefact in $msi, $zip, $recipes) {
    if (-not (Test-Path $artefact)) { throw "$artefact is missing; run scripts\build.ps1 release first" }
}

# Checksums, in the shape sha256sum reads back.
$sums = foreach ($artefact in $msi, $zip, $recipes) {
    '{0}  {1}' -f (Get-FileHash $artefact -Algorithm SHA256).Hash.ToLower(), (Split-Path $artefact -Leaf)
}
Set-Content -Path (Join-Path $dist 'SHA256SUMS.txt') -Value $sums -Encoding ascii

# The link names the commit the binaries were built from; asked of the
# binary so the notes cannot disagree with it.
$stamp = & (Join-Path $dist "GameModeExecutor-$version\gamemode-executor.exe") --version
$docs = ($stamp | Select-String -Pattern '^documentation:\s+(\S+)').Matches[0].Groups[1].Value

# The previous tag by version order, not by date. Indexed rather than piped
# into Select-Object -First, which stops the upstream command.
$tags = @(& git -C $root tag --sort=-v:refname | Where-Object { $_ -ne $Tag -and $_ -match '^v\d+\.\d+\.\d+$' })
$previous = if ($tags.Count) { $tags[0] } else { $null }
$commits = if ($previous) {
    $log = @(& git -C $root log --format='- %s' "$previous..$Tag" --no-merges)
    "## For the curious`n`nThe commits since $previous, newest first:`n`n" + ($log -join "`n")
} else {
    "## For the curious`n`nThe first release: every commit is in it."
}

$lines = @(
    'Runs the executables you configure when a game starts and when it stops. Measured on Windows 11; it relies on the Xbox Game Bar component Windows ships by default, which Windows 10 carries too since version 1903, but nobody has run it there yet.',
    '',
    '## Install',
    '',
    "- **GameModeExecutor-$version.msi** -- the installer. Per user, no administrator prompt, into ``%LOCALAPPDATA%\Programs\GameModeExecutor``. It writes a starter configuration if you have none, registers the logon task and starts the watcher: the icon beside the clock is the confirmation. Then right-click it, *Edit configuration*.",
    "- **GameModeExecutor-$version.zip** -- the same executables, to unpack wherever you like. Then ``gamemode-executor init``, edit the file it wrote, ``gamemode-executor install-task``.",
    "- **GameModeExecutor-recipes-$version.zip** -- the worked examples, one folder each, for this build.",
    '',
    "[Documentation for this exact build]($docs).",
    '',
    '## What changed',
    '',
    $notes,
    '',
    $commits,
    '',
    '## SHA-256',
    '',
    '```'
) + $sums + @(
    '```'
)
Set-Content -Path (Join-Path $dist 'notes.md') -Value $lines -Encoding utf8
Get-Content (Join-Path $dist 'notes.md')
