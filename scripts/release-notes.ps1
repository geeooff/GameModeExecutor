# Writes the release notes for a tag, from what the release build produced:
# the documentation link the binary carries, the checksums of the two
# artefacts, and the commits since the previous tag. Run by the release
# workflow after scripts\build.ps1 release; nothing in it needs GitHub.
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
$msi = Join-Path $dist "GameModeExecutor-$version.msi"
$zip = Join-Path $dist "GameModeExecutor-$version.zip"
foreach ($artefact in $msi, $zip) {
    if (-not (Test-Path $artefact)) { throw "$artefact is missing; run scripts\build.ps1 release first" }
}

# Checksums, in the shape sha256sum reads back.
$sums = foreach ($artefact in $msi, $zip) {
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
$changes = if ($previous) {
    $log = @(& git -C $root log --format='- %s' "$previous..$Tag")
    "## Changes since $previous`n`n" + ($log -join "`n")
} else {
    "## Changes`n`nFirst public release."
}

$lines = @(
    'Runs the executables you configure when a game starts and when it stops. Measured on Windows 11; it relies on the Xbox Game Bar component Windows ships by default, which Windows 10 carries too since version 1903, but nobody has run it there yet.',
    '',
    '## Install',
    '',
    "- **GameModeExecutor-$version.msi** -- the installer. Per user, no administrator prompt, into ``%LOCALAPPDATA%\Programs\GameModeExecutor``. It writes a starter configuration if you have none, registers the logon task and starts the watcher: the icon beside the clock is the confirmation. Then right-click it, *Edit configuration*.",
    "- **GameModeExecutor-$version.zip** -- the same executables, to unpack wherever you like. Then ``gamemode-executor init``, edit the file it wrote, ``gamemode-executor install-task``.",
    '',
    "[Documentation for this exact build]($docs).",
    '',
    '## SHA-256',
    '',
    '```'
) + $sums + @(
    '```',
    '',
    $changes
)
Set-Content -Path (Join-Path $dist 'notes.md') -Value $lines -Encoding utf8
Get-Content (Join-Path $dist 'notes.md')
