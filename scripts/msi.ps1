# Builds the Windows Installer package from a staged release folder.
#
# Per-user, no elevation, no UI: the two executables, the license and the
# readme -- the same four files the zip carries -- go to
# %LOCALAPPDATA%\Programs\GameModeExecutor. The user's configuration, log,
# marker and scheduled tasks are not components, so no repair, upgrade or
# uninstall reaches them. Four custom actions, all the program's own
# commands and all idempotent, run through the windowless executable:
# `stop` before an uninstall or upgrade touches the files, so the Restart
# Manager never has to ask -- with `--handover` on an upgrade, so a game
# session in progress is resumed by the new watcher rather than closed and
# reopened; `init`, which writes a starter configuration
# only where there is none, and `install-task`, which registers the logon
# task only where there is none and then starts the watcher -- the icon
# appearing is the confirmation -- to finish an install or upgrade; and
# `uninstall-task` on an uninstall, since the task is the package's to take
# down.
#
# Written with nothing but Windows Installer's own COM automation and
# makecab, so a stock runner can build it -- docs/design/08-distribution.md
# records the measurements behind every choice here.
#
# Identifiers are deterministic: the product code derives from the version,
# the package code from the version and the commit, each component from its
# file name. Two builds of the same commit give the same package.
param(
    [Parameter(Mandatory)] [string] $Stage,    # holds the executables, LICENSE.txt and README.txt
    [Parameter(Mandatory)] [string] $Version,  # x.y.z, from Cargo.toml
    [Parameter(Mandatory)] [string] $Out,      # the .msi to write
    [string] $Commit = 'unknown',
    [string] $DocumentationUrl = 'https://github.com/Geeooff/GameModeExecutor',
    [string] $Icon = '',                       # .ico for Programs and Features
    # Tests only: a package that is not the real product. It gets its own
    # upgrade and product codes and a name that says so, and can be installed
    # beside the real one without either seeing the other.
    [string] $Family = ''
)
$ErrorActionPreference = 'Stop'

# Fixed for the life of the product: every version shares it, which is how
# a newer package finds the older installation it replaces.
$UpgradeCode = '{8C4E0B2D-3F6A-4E7B-9A1C-5D2E8F7B6A30}'
$ProductName = 'GameModeExecutor'
$Namespace   = [guid] '{2B7D6F1E-9C4A-4D3B-8E5F-1A6C9D0B7E42}'
$Author      = 'Geoffrey Vancoetsem'
$Repository  = 'https://github.com/Geeooff/GameModeExecutor'

# A name-based GUID (the SHA-1 construction of RFC 4122 section 4.3),
# braced and uppercase as Windows Installer wants it.
function New-NameGuid([string] $Name) {
    $sha = [System.Security.Cryptography.SHA1]::Create()
    $ns = $Namespace.ToByteArray()
    # RFC 4122 hashes the namespace in network order; .NET stores the first
    # three fields little-endian, so swap them first.
    [Array]::Reverse($ns, 0, 4); [Array]::Reverse($ns, 4, 2); [Array]::Reverse($ns, 6, 2)
    $hash = $sha.ComputeHash($ns + [System.Text.Encoding]::UTF8.GetBytes($Name))
    $bytes = $hash[0..15]
    $bytes[6] = ($bytes[6] -band 0x0F) -bor 0x50   # version 5
    $bytes[8] = ($bytes[8] -band 0x3F) -bor 0x80   # RFC 4122 variant
    [Array]::Reverse($bytes, 0, 4); [Array]::Reverse($bytes, 4, 2); [Array]::Reverse($bytes, 6, 2)
    return '{' + ([guid] [byte[]] $bytes).ToString().ToUpperInvariant() + '}'
}

if ($Family) {
    $UpgradeCode = New-NameGuid "upgrade/$Family"
    $ProductName = "GameModeExecutor ($Family)"
}
# The real product's names stay exactly what they were before families
# existed, so its codes do not move.
$scope = if ($Family) { "$Family/" } else { '' }
$ProductCode = New-NameGuid "product/$scope$Version"
$PackageCode = New-NameGuid "package/$scope$Version/$Commit"

# --- the files ---------------------------------------------------------------
# Keys are Windows Installer identifiers (no hyphens), and the cabinet entries
# carry the same names. Short names must be unique 8.3 names.
$files = @(
    @{ Key = 'gamemode_executor.exe';  Name = 'gamemode-executor.exe';  Short = 'GAMEMO~1.EXE' },
    @{ Key = 'gamemode_executorw.exe'; Name = 'gamemode-executorw.exe'; Short = 'GAMEMO~2.EXE' },
    @{ Key = 'LICENSE.txt';            Name = 'LICENSE.txt';            Short = 'LICENSE.TXT' },
    @{ Key = 'README.txt';             Name = 'README.txt';             Short = 'README.TXT' }
)
foreach ($f in $files) {
    $f.Path = Join-Path $Stage $f.Name
    if (-not (Test-Path $f.Path)) { throw "$($f.Name) is not in $Stage" }
    $item = Get-Item $f.Path
    $f.Size = [int] $item.Length
    $raw = $item.VersionInfo.FileVersionRaw
    $f.Version = if ($raw -and $raw -ne [version] '0.0.0.0') { $raw.ToString() } else { '' }
    $f.Component = 'C_' + ($f.Key -replace '\.', '_')
    $f.Guid = New-NameGuid "component/$($f.Name)"
}

$work = Join-Path ([System.IO.Path]::GetTempPath()) "gamemode-executor-msi-$([guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory $work | Out-Null
try {
    # --- the cabinet ---------------------------------------------------------
    $ddf = @(
        '.OPTION EXPLICIT',
        '.Set CabinetNameTemplate=files.cab',
        ".Set DiskDirectoryTemplate=$work",
        '.Set CompressionType=LZX',
        '.Set Cabinet=on',
        '.Set Compress=on',
        '.Set InfFileName=nul',
        '.Set RptFileName=nul'
    )
    $sequence = 0
    foreach ($f in $files) {
        $sequence++
        $f.Sequence = $sequence
        $ddf += "`"$($f.Path)`" $($f.Key)"
    }
    $ddfPath = Join-Path $work 'files.ddf'
    Set-Content -Path $ddfPath -Value $ddf -Encoding ASCII
    $null = & makecab.exe /F $ddfPath
    $cab = Join-Path $work 'files.cab'
    if (-not (Test-Path $cab)) { throw 'makecab produced no cabinet' }

    # --- the database --------------------------------------------------------
    $installer = New-Object -ComObject WindowsInstaller.Installer
    function Invoke-Com($object, $method, $arguments) {
        return $object.GetType().InvokeMember($method, 'InvokeMethod', $null, $object, [object[]] $arguments)
    }
    function Get-ComProperty($object, $name, $arguments) {
        return $object.GetType().InvokeMember($name, 'GetProperty', $null, $object, [object[]] $arguments)
    }
    function Set-ComProperty($object, $name, $arguments) {
        $object.GetType().InvokeMember($name, 'SetProperty', $null, $object, [object[]] $arguments) | Out-Null
    }
    if (Test-Path $Out) { Remove-Item $Out -Force }
    $db = Invoke-Com $installer 'OpenDatabase' @($Out, 3)   # msiOpenDatabaseModeCreate

    function Exec-Sql([string] $sql) {
        try { $view = Invoke-Com $db 'OpenView' @($sql) } catch { throw "bad SQL: $sql" }
        Invoke-Com $view 'Execute' @() | Out-Null
        Invoke-Com $view 'Close' @() | Out-Null
    }
    # MSI SQL insists on the column list in an INSERT.
    $columns = @{
        Property               = 'Property, Value'
        Directory              = 'Directory, Directory_Parent, DefaultDir'
        Component              = 'Component, ComponentId, Directory_, Attributes, Condition, KeyPath'
        File                   = 'File, Component_, FileName, FileSize, Version, Language, Attributes, Sequence'
        MsiFileHash            = 'File_, Options, HashPart1, HashPart2, HashPart3, HashPart4'
        Feature                = 'Feature, Feature_Parent, Title, Description, Display, Level, Directory_, Attributes'
        FeatureComponents      = 'Feature_, Component_'
        Media                  = 'DiskId, LastSequence, DiskPrompt, Cabinet, VolumeLabel, Source'
        InstallExecuteSequence = 'Action, Condition, Sequence'
        InstallUISequence      = 'Action, Condition, Sequence'
        AdminExecuteSequence   = 'Action, Condition, Sequence'
        AdminUISequence        = 'Action, Condition, Sequence'
        AdvtExecuteSequence    = 'Action, Condition, Sequence'
        Upgrade                = 'UpgradeCode, VersionMin, VersionMax, Language, Attributes, Remove, ActionProperty'
        LaunchCondition        = 'Condition, Description'
        CustomAction           = 'Action, Type, Source, Target, ExtendedType'
        Icon                   = 'Name, Data'
        _Validation            = 'Table, Column, Nullable, MinValue, MaxValue, KeyTable, KeyColumn, Category, Set, Description'
        _Streams               = 'Name, Data'
    }
    function Insert([string] $table, [object[]] $values) {
        $marks = ($values | ForEach-Object { '?' }) -join ', '
        $cols = ($columns[$table] -split ', ' | ForEach-Object { "``$_``" }) -join ', '
        try { $view = Invoke-Com $db 'OpenView' @("INSERT INTO ``$table`` ($cols) VALUES ($marks)") } catch { throw "bad INSERT into $table" }
        $record = Invoke-Com $installer 'CreateRecord' @($values.Count)
        for ($i = 0; $i -lt $values.Count; $i++) {
            $v = $values[$i]
            if ($v -is [int]) { Set-ComProperty $record 'IntegerData' @(($i + 1), $v) }
            elseif ($v -is [string] -and $v.StartsWith('stream:')) { Invoke-Com $record 'SetStream' @(($i + 1), $v.Substring(7)) | Out-Null }
            elseif ($null -eq $v -or $v -eq '') { }   # stays null
            else { Set-ComProperty $record 'StringData' @(($i + 1), [string] $v) }
        }
        Invoke-Com $view 'Execute' @($record) | Out-Null
        Invoke-Com $view 'Close' @() | Out-Null
        # Released at once: a record holding a stream keeps the package open,
        # and the caller may well hand the package to msiexec next.
        [System.Runtime.InteropServices.Marshal]::ReleaseComObject($record) | Out-Null
        [System.Runtime.InteropServices.Marshal]::ReleaseComObject($view) | Out-Null
    }

    Exec-Sql "CREATE TABLE ``Property`` (``Property`` CHAR(72) NOT NULL, ``Value`` LONGCHAR NOT NULL LOCALIZABLE PRIMARY KEY ``Property``)"
    Exec-Sql "CREATE TABLE ``Directory`` (``Directory`` CHAR(72) NOT NULL, ``Directory_Parent`` CHAR(72), ``DefaultDir`` CHAR(255) NOT NULL LOCALIZABLE PRIMARY KEY ``Directory``)"
    Exec-Sql "CREATE TABLE ``Component`` (``Component`` CHAR(72) NOT NULL, ``ComponentId`` CHAR(38), ``Directory_`` CHAR(72) NOT NULL, ``Attributes`` SHORT NOT NULL, ``Condition`` CHAR(255), ``KeyPath`` CHAR(72) PRIMARY KEY ``Component``)"
    Exec-Sql "CREATE TABLE ``File`` (``File`` CHAR(72) NOT NULL, ``Component_`` CHAR(72) NOT NULL, ``FileName`` CHAR(255) NOT NULL LOCALIZABLE, ``FileSize`` LONG NOT NULL, ``Version`` CHAR(72), ``Language`` CHAR(20), ``Attributes`` SHORT, ``Sequence`` LONG NOT NULL PRIMARY KEY ``File``)"
    Exec-Sql "CREATE TABLE ``MsiFileHash`` (``File_`` CHAR(72) NOT NULL, ``Options`` SHORT NOT NULL, ``HashPart1`` LONG NOT NULL, ``HashPart2`` LONG NOT NULL, ``HashPart3`` LONG NOT NULL, ``HashPart4`` LONG NOT NULL PRIMARY KEY ``File_``)"
    Exec-Sql "CREATE TABLE ``Feature`` (``Feature`` CHAR(38) NOT NULL, ``Feature_Parent`` CHAR(38), ``Title`` CHAR(64) LOCALIZABLE, ``Description`` CHAR(255) LOCALIZABLE, ``Display`` SHORT, ``Level`` SHORT NOT NULL, ``Directory_`` CHAR(72), ``Attributes`` SHORT NOT NULL PRIMARY KEY ``Feature``)"
    Exec-Sql "CREATE TABLE ``FeatureComponents`` (``Feature_`` CHAR(38) NOT NULL, ``Component_`` CHAR(72) NOT NULL PRIMARY KEY ``Feature_``, ``Component_``)"
    Exec-Sql "CREATE TABLE ``Media`` (``DiskId`` SHORT NOT NULL, ``LastSequence`` LONG NOT NULL, ``DiskPrompt`` CHAR(64) LOCALIZABLE, ``Cabinet`` CHAR(255), ``VolumeLabel`` CHAR(32), ``Source`` CHAR(72) PRIMARY KEY ``DiskId``)"
    foreach ($t in 'InstallExecuteSequence', 'InstallUISequence', 'AdminExecuteSequence', 'AdminUISequence', 'AdvtExecuteSequence') {
        Exec-Sql "CREATE TABLE ``$t`` (``Action`` CHAR(72) NOT NULL, ``Condition`` CHAR(255), ``Sequence`` SHORT PRIMARY KEY ``Action``)"
    }
    Exec-Sql "CREATE TABLE ``Upgrade`` (``UpgradeCode`` CHAR(38) NOT NULL, ``VersionMin`` CHAR(20), ``VersionMax`` CHAR(20), ``Language`` CHAR(255), ``Attributes`` LONG NOT NULL, ``Remove`` CHAR(255), ``ActionProperty`` CHAR(72) NOT NULL PRIMARY KEY ``UpgradeCode``, ``VersionMin``, ``VersionMax``, ``Language``, ``Attributes``)"
    Exec-Sql "CREATE TABLE ``LaunchCondition`` (``Condition`` CHAR(255) NOT NULL, ``Description`` CHAR(255) NOT NULL LOCALIZABLE PRIMARY KEY ``Condition``)"
    Exec-Sql "CREATE TABLE ``CustomAction`` (``Action`` CHAR(72) NOT NULL, ``Type`` SHORT NOT NULL, ``Source`` CHAR(72), ``Target`` CHAR(255), ``ExtendedType`` LONG PRIMARY KEY ``Action``)"
    Exec-Sql "CREATE TABLE ``Icon`` (``Name`` CHAR(72) NOT NULL, ``Data`` OBJECT NOT NULL PRIMARY KEY ``Name``)"
    Exec-Sql "CREATE TABLE ``_Validation`` (``Table`` CHAR(32) NOT NULL, ``Column`` CHAR(32) NOT NULL, ``Nullable`` CHAR(4) NOT NULL, ``MinValue`` LONG, ``MaxValue`` LONG, ``KeyTable`` CHAR(255), ``KeyColumn`` SHORT, ``Category`` CHAR(32), ``Set`` CHAR(255), ``Description`` CHAR(255) PRIMARY KEY ``Table``, ``Column``)"

    # Per-user takes all three, measured 2026-09-17 on an administrator
    # account running unelevated: the summary stream's "no elevation
    # required" bit (below) lets the install run without a prompt;
    # ALLUSERS=2 with MSIINSTALLPERUSER=1 -- Single Package Authoring, as
    # documented -- resolves to a per-user install and redirects
    # ProgramFilesFolder to %LOCALAPPDATA%\Programs. With the bit alone the
    # folder stays at C:\Program Files (x86); with ALLUSERS=2 alone the
    # install turns per-machine for an administrator and fails unelevated.
    # The log's "MSIINSTALLPERUSER ... Ignoring" line is misleading: the
    # property still decides how ALLUSERS=2 resolves. The ARP entries are
    # what Programs and Features shows.
    $properties = [ordered] @{
        ProductCode            = $ProductCode
        UpgradeCode            = $UpgradeCode
        ProductName            = $ProductName
        ProductVersion         = $Version
        ProductLanguage        = '1033'
        Manufacturer           = $Author
        ALLUSERS               = '2'
        MSIINSTALLPERUSER      = '1'
        ARPCOMMENTS            = 'Runs the executables you configure when a game starts and when it stops.'
        ARPURLINFOABOUT        = $Repository
        ARPHELPLINK            = $DocumentationUrl
        ARPNOMODIFY            = '1'
        SecureCustomProperties = 'PREVIOUSVERSIONS;NEWERVERSIONDETECTED'
    }
    if ($Icon -and (Test-Path $Icon)) {
        Insert 'Icon' @('GameModeExecutor.ico', "stream:$Icon")
        $properties.ARPPRODUCTICON = 'GameModeExecutor.ico'
    }
    foreach ($k in $properties.Keys) { Insert 'Property' @($k, $properties[$k]) }

    # ProgramFilesFolder, which Windows Installer redirects to
    # %LOCALAPPDATA%\Programs for a per-user package -- the documented
    # mechanism, and measured 2026-09-17. Not ProgramFiles64Folder: that one
    # stays at C:\Program Files, where an unelevated install cannot write. And
    # not the profile folder spelled out: ICE38, ICE64 and ICE91 then demand
    # registry key paths and RemoveFile rows meant for packages that might be
    # installed per machine, which this one never is.
    Insert 'Directory' @('TARGETDIR', $null, 'SourceDir')
    Insert 'Directory' @('ProgramFilesFolder', 'TARGETDIR', 'PFiles')
    Insert 'Directory' @('INSTALLDIR', 'ProgramFilesFolder', 'GAMEMO~1|GameModeExecutor')

    Insert 'Feature' @('Main', $null, 'GameModeExecutor', 'The watcher and its command line.', 1, 1, 'INSTALLDIR', 0)
    foreach ($f in $files) {
        # Attributes 0, a 32-bit component although the executables are
        # 64-bit: the bit only governs registry reflection, which nothing
        # here uses, and ICE80 refuses 64-bit components in ProgramFilesFolder.
        Insert 'Component' @($f.Component, $f.Guid, 'INSTALLDIR', 0, $null, $f.Key)
        $name = if ($f.Short -eq $f.Name) { $f.Name } else { "$($f.Short)|$($f.Name)" }
        $language = if ($f.Version) { '1033' } else { $null }
        # 512: vital -- the install fails rather than continues without it.
        Insert 'File' @($f.Key, $f.Component, $name, $f.Size, $f.Version, $language, 512, $f.Sequence)
        Insert 'FeatureComponents' @('Main', $f.Component)
        if (-not $f.Version) {
            # Unversioned files are compared by hash on repair and upgrade,
            # not by date, when the package carries one.
            $hash = $installer.FileHash([string] $f.Path, [int] 0)
            $parts = 1..4 | ForEach-Object { [int] $hash.IntegerData([int] $_) }
            Insert 'MsiFileHash' @($f.Key, 0, $parts[0], $parts[1], $parts[2], $parts[3])
        }
    }
    Insert 'Media' @(1, $sequence, $null, '#files.cab', $null, $null)
    Insert '_Streams' @('files.cab', "stream:$cab")

    # Older versions are removed first -- RemoveExistingProducts right after
    # InstallInitialize -- so the new files never land beside old ones. A
    # newer version already installed stops the install before it starts.
    Insert 'Upgrade' @($UpgradeCode, '0.0.0', $Version, $null, 256, $null, 'PREVIOUSVERSIONS')   # 256: min inclusive, max exclusive
    Insert 'Upgrade' @($UpgradeCode, $Version, $null, $null, 2, $null, 'NEWERVERSIONDETECTED')  # 2: detect only; min exclusive
    Insert 'LaunchCondition' @('NOT NEWERVERSIONDETECTED', 'A newer version of GameModeExecutor is already installed.')

    # 1042 = 18 (run an executable from the File table) + 1024 (deferred,
    # in the install script, after the files are on disk). Impersonated, as
    # deferred actions are by default, so they act as the user -- which is
    # the only way a per-user package may run anything. The windowless twin,
    # because Windows Installer does not hide an action's console and the
    # console binary would flash one twice (seen 2026-09-18).
    Insert 'CustomAction' @('InitConfig', 1042, 'gamemode_executorw.exe', 'init', $null)
    Insert 'CustomAction' @('RegisterTask', 1042, 'gamemode_executorw.exe', 'install-task', $null)
    # The task is the package's infrastructure, not the user's data: left
    # behind, it would start a missing executable at every logon and fail
    # (the maintainer's remark on 2026-09-18). Removed on an uninstall, from
    # the executable it runs before RemoveFiles takes it; kept through an
    # upgrade, which re-registers nothing and so keeps a delay or a path the
    # user chose.
    Insert 'CustomAction' @('UnregisterTask', 1042, 'gamemode_executorw.exe', 'uninstall-task', $null)
    # 98 = 34 (run a command line in a directory from the Directory table)
    # + 64 (carry on if it fails). Immediate, before InstallValidate, where
    # the Restart Manager would otherwise find the watcher holding the files
    # and put up its "close these applications" dialog (seen 2026-09-18 on
    # the first uninstall). Both run the *installed* executable, which an
    # upgrade has not replaced yet. An upgrade hands a game session over --
    # RegisterTask starts the new watcher seconds later and it resumes the
    # session, nothing runs twice; a removal restores, since nobody follows.
    # An installed version too old to know the verb or the flag fails the
    # action, which continues, and the dialog comes back for that one
    # upgrade: 0.1.0 does not know --handover, accepted 2026-09-18.
    Insert 'CustomAction' @('StopForUpgrade', 98, 'INSTALLDIR', '"[INSTALLDIR]gamemode-executorw.exe" stop --handover', $null)
    Insert 'CustomAction' @('StopForRemoval', 98, 'INSTALLDIR', '"[INSTALLDIR]gamemode-executorw.exe" stop', $null)

    $sequences = @{
        InstallExecuteSequence = @(
            @('FindRelatedProducts', 25), @('LaunchConditions', 100), @('ValidateProductID', 700),
            @('CostInitialize', 800), @('FileCost', 900), @('CostFinalize', 1000),
            @('StopForUpgrade', 1300), @('StopForRemoval', 1310),
            @('InstallValidate', 1400), @('InstallInitialize', 1500),
            @('RemoveExistingProducts', 1510),
            @('ProcessComponents', 1600), @('UnpublishFeatures', 1800),
            @('UnregisterTask', 3400), @('RemoveFiles', 3500), @('InstallFiles', 4000),
            @('InitConfig', 4100), @('RegisterTask', 4200),
            @('RegisterUser', 6000), @('RegisterProduct', 6100),
            @('PublishFeatures', 6300), @('PublishProduct', 6400),
            @('InstallFinalize', 6600)
        )
        InstallUISequence = @(
            @('FindRelatedProducts', 25), @('LaunchConditions', 100),
            @('CostInitialize', 800), @('FileCost', 900), @('CostFinalize', 1000), @('ExecuteAction', 1300)
        )
        AdminExecuteSequence = @(
            @('CostInitialize', 800), @('FileCost', 900), @('CostFinalize', 1000),
            @('InstallValidate', 1400), @('InstallInitialize', 1500),
            @('InstallAdminPackage', 3900), @('InstallFiles', 4000), @('InstallFinalize', 6600)
        )
        AdminUISequence = @(
            @('CostInitialize', 800), @('FileCost', 900), @('CostFinalize', 1000), @('ExecuteAction', 1300)
        )
        AdvtExecuteSequence = @(
            @('CostInitialize', 800), @('CostFinalize', 1000), @('InstallValidate', 1400), @('InstallInitialize', 1500),
            @('PublishFeatures', 6300), @('PublishProduct', 6400), @('InstallFinalize', 6600)
        )
    }
    # The setup actions run on an install and on an upgrade -- a new product
    # code is not Installed -- and never on a repair or an uninstall. The
    # watcher is stopped on an upgrade and on an uninstall, where its files
    # are about to go; a fresh install has none to stop. Not when this
    # product is the old one being removed by an upgrade: the new package
    # stopped the watcher before it got here, and the first upgrade
    # (2026-09-18) logged a second, empty stop for nothing.
    $conditions = @{
        InitConfig     = 'NOT Installed'
        RegisterTask   = 'NOT Installed'
        StopForUpgrade = 'PREVIOUSVERSIONS'
        StopForRemoval = 'REMOVE="ALL" AND NOT UPGRADINGPRODUCTCODE'
        UnregisterTask = 'REMOVE="ALL" AND NOT UPGRADINGPRODUCTCODE'
    }
    foreach ($t in $sequences.Keys) {
        foreach ($row in $sequences[$t]) {
            $condition = if ($conditions.ContainsKey($row[0])) { $conditions[$row[0]] } else { $null }
            Insert $t @($row[0], $condition, [int] $row[1])
        }
    }

    # The column specifications ICE03 checks against, for the tables above
    # only. Copied from the SDK's schema database (orca.dat) on 2026-09-17.
    $validation = @'
_Validation | Table | N |  |  |  |  | Identifier |  | Name of table
_Validation | Column | N |  |  |  |  | Identifier |  | Name of column
_Validation | Description | Y |  |  |  |  | Text |  | Description of column
_Validation | Set | Y |  |  |  |  | Text |  | Set of values that are permitted
_Validation | Category | Y |  |  |  |  |  | Text;Formatted;Template;Condition;Guid;Path;Version;Language;Identifier;Binary;UpperCase;LowerCase;Filename;Paths;AnyPath;WildCardFilename;RegPath;KeyFormatted;CustomSource;Property;Cabinet;Shortcut;URL | String category
_Validation | KeyColumn | Y | 1 | 32 |  |  |  |  | Column to which foreign key connects
_Validation | KeyTable | Y |  |  |  |  | Identifier |  | For foreign key, Name of table to which data must link
_Validation | MaxValue | Y | -2147483647 | 2147483647 |  |  |  |  | Maximum value allowed
_Validation | MinValue | Y | -2147483647 | 2147483647 |  |  |  |  | Minimum value allowed
_Validation | Nullable | N |  |  |  |  |  | Y;N;@ | Whether the column is nullable
AdminExecuteSequence | Action | N |  |  |  |  | Identifier |  | Name of action to invoke, either built-in or custom.
AdminExecuteSequence | Condition | Y |  |  |  |  | Condition |  | Optional expression which skips the action if evaluates to expFalse.If the expression syntax is invalid, the engine will terminate, returning iesBadActionData.
AdminExecuteSequence | Sequence | Y | -4 | 32767 |  |  |  |  | Number that determines the sort order in which the actions are to be executed.  Leave blank to suppress action.
AdminUISequence | Action | N |  |  |  |  | Identifier |  | Name of action to invoke, either built-in or custom.
AdminUISequence | Condition | Y |  |  |  |  | Condition |  | Optional expression which skips the action if evaluates to expFalse.If the expression syntax is invalid, the engine will terminate, returning iesBadActionData.
AdminUISequence | Sequence | Y | -4 | 32767 |  |  |  |  | Number that determines the sort order in which the actions are to be executed.  Leave blank to suppress action.
AdvtExecuteSequence | Action | N |  |  |  |  | Identifier |  | Name of action to invoke, either built-in or custom.
AdvtExecuteSequence | Condition | Y |  |  |  |  | Condition |  | Optional expression which skips the action if evaluates to expFalse.If the expression syntax is invalid, the engine will terminate, returning iesBadActionData.
AdvtExecuteSequence | Sequence | Y | -4 | 32767 |  |  |  |  | Number that determines the sort order in which the actions are to be executed.  Leave blank to suppress action.
Component | Attributes | N |  |  |  |  |  |  | Remote execution option, one of irsEnum
CustomAction | Action | N |  |  |  |  | Identifier |  | Primary key, name of action, normally appears in sequence table unless private use.
CustomAction | ExtendedType | Y | 0 | 2147483647 |  |  |  |  | The numeric custom action type info flags.
CustomAction | Source | Y |  |  |  |  | CustomSource |  | The table reference of the source of the code.
CustomAction | Target | Y |  |  |  |  | Formatted |  | Excecution parameter, depends on the type of custom action
CustomAction | Type | N | 1 | 32767 |  |  |  |  | The numeric custom action type, consisting of source location, code type, entry, option flags.
Component | Component | N |  |  |  |  | Identifier |  | Primary key used to identify a particular component record.
Component | ComponentId | Y |  |  |  |  | Guid |  | A string GUID unique to this component, version, and language.
Component | Condition | Y |  |  |  |  | Condition |  | A conditional statement that will disable this component if the specified condition evaluates to the 'True' state. If a component is disabled, it will not be installed, regardless of the 'Action' state associated with the component.
Component | Directory_ | N |  |  | Directory | 1 | Identifier |  | Required key of a Directory table record. This is actually a property name whose value contains the actual path, set either by the AppSearch action or with the default setting obtained from the Directory table.
Component | KeyPath | Y |  |  | File;Registry;ODBCDataSource | 1 | Identifier |  | Either the primary key into the File table, Registry table, or ODBCDataSource table. This extract path is stored when the component is installed, and is used to detect the presence of the component and to return the path to it.
Directory | DefaultDir | N |  |  |  |  | DefaultDir |  | The default sub-path under parent's path.
Directory | Directory | N |  |  |  |  | Identifier |  | Unique identifier for directory entry, primary key. If a property by this name is defined, it contains the full path to the directory.
Directory | Directory_Parent | Y |  |  | Directory | 1 | Identifier |  | Reference to the entry in this table specifying the default parent directory. A record parented to itself or with a Null parent represents a root of the install tree.
Feature | Attributes | N |  |  |  |  |  | 0;1;2;4;5;6;8;9;10;16;17;18;20;21;22;24;25;26;32;33;34;36;37;38;48;49;50;52;53;54 | Feature attributes
Feature | Description | Y |  |  |  |  | Text |  | Longer descriptive text describing a visible feature item.
Feature | Directory_ | Y |  |  | Directory | 1 | UpperCase |  | The name of the Directory that can be configured by the UI. A non-null value will enable the browse button.
Feature | Display | Y | 0 | 32767 |  |  |  |  | Numeric sort order, used to force a specific display ordering.
Feature | Feature | N |  |  |  |  | Identifier |  | Primary key used to identify a particular feature record.
Feature | Feature_Parent | Y |  |  | Feature | 1 | Identifier |  | Optional key of a parent record in the same table. If the parent is not selected, then the record will not be installed. Null indicates a root item.
Feature | Level | N | 0 | 32767 |  |  |  |  | The install level at which record will be initially selected. An install level of 0 will disable an item and prevent its display.
Feature | Title | Y |  |  |  |  | Text |  | Short text identifying a visible feature item.
FeatureComponents | Component_ | N |  |  | Component | 1 | Identifier |  | Foreign key into Component table.
FeatureComponents | Feature_ | N |  |  | Feature | 1 | Identifier |  | Foreign key into Feature table.
File | Attributes | Y | 0 | 32767 |  |  |  |  | Integer containing bit flags representing file attributes (with the decimal value of each bit position in parentheses)
File | Component_ | N |  |  | Component | 1 | Identifier |  | Foreign key referencing Component that controls the file.
File | File | N |  |  |  |  | Identifier |  | Primary key, non-localized token, must match identifier in cabinet.  For uncompressed files, this field is ignored.
File | FileName | N |  |  |  |  | Filename |  | File name used for installation, may be localized.  This may contain a "short name|long name" pair.
File | FileSize | N | 0 | 2147483647 |  |  |  |  | Size of file in bytes (long integer).
File | Language | Y |  |  |  |  | Language |  | List of decimal language Ids, comma-separated if more than one.
File | Sequence | N | 1 | 32767 |  |  |  |  | Sequence with respect to the media images; order must track cabinet order.
File | Version | Y |  |  | File | 1 | Version |  | Version string for versioned files;  Blank for unversioned files.
Icon | Data | N |  |  |  |  | Binary |  | Binary stream. The binary icon data in PE (.DLL or .EXE) or icon (.ICO) format.
Icon | Name | N |  |  |  |  | Identifier |  | Primary key. Name of the icon file.
InstallExecuteSequence | Action | N |  |  |  |  | Identifier |  | Name of action to invoke, either built-in or custom.
InstallExecuteSequence | Condition | Y |  |  |  |  | Condition |  | Optional expression which skips the action if evaluates to expFalse.If the expression syntax is invalid, the engine will terminate, returning iesBadActionData.
InstallExecuteSequence | Sequence | Y | -4 | 32767 |  |  |  |  | Number that determines the sort order in which the actions are to be executed.  Leave blank to suppress action.
InstallUISequence | Action | N |  |  |  |  | Identifier |  | Name of action to invoke, either built-in or custom.
InstallUISequence | Condition | Y |  |  |  |  | Condition |  | Optional expression which skips the action if evaluates to expFalse.If the expression syntax is invalid, the engine will terminate, returning iesBadActionData.
InstallUISequence | Sequence | Y | -4 | 32767 |  |  |  |  | Number that determines the sort order in which the actions are to be executed.  Leave blank to suppress action.
LaunchCondition | Condition | N |  |  |  |  | Condition |  | Expression which must evaluate to TRUE in order for install to commence.
LaunchCondition | Description | N |  |  |  |  | Formatted |  | Localizable text to display when condition fails and install must abort.
Media | Cabinet | Y |  |  |  |  | Cabinet |  | If some or all of the files stored on the media are compressed in a cabinet, the name of that cabinet.
Media | DiskId | N | 1 | 32767 |  |  |  |  | Primary key, integer to determine sort order for table.
Media | DiskPrompt | Y |  |  |  |  | Text |  | Disk name: the visible text actually printed on the disk.  This will be used to prompt the user when this disk needs to be inserted.
Media | LastSequence | N | 0 | 32767 |  |  |  |  | File sequence number for the last file for this media.
Media | Source | Y |  |  |  |  | Property |  | The property defining the location of the cabinet file.
Media | VolumeLabel | Y |  |  |  |  | Text |  | The label attributed to the volume.
MsiFileHash | File_ | N |  |  | File | 1 | Identifier |  | Primary key, foreign key into File table referencing file with this hash
MsiFileHash | HashPart1 | N |  |  |  |  |  |  | Size of file in bytes (long integer).
MsiFileHash | HashPart2 | N |  |  |  |  |  |  | Size of file in bytes (long integer).
MsiFileHash | HashPart3 | N |  |  |  |  |  |  | Size of file in bytes (long integer).
MsiFileHash | HashPart4 | N |  |  |  |  |  |  | Size of file in bytes (long integer).
MsiFileHash | Options | N | 0 | 32767 |  |  |  |  | Various options and attributes for this hash.
Property | Property | N |  |  |  |  | Identifier |  | Name of property, uppercase if settable by launcher or loader.
Property | Value | N |  |  |  |  | Text |  | String value for property.  Never null or empty.
Upgrade | ActionProperty | N |  |  |  |  | UpperCase |  | The property to set when a product in this set is found.
Upgrade | Attributes | N | 0 | 2147483647 |  |  |  |  | The attributes of this product set.
Upgrade | Language | Y |  |  |  |  | Language |  | A comma-separated list of languages for either products in this set or products not in this set.
Upgrade | Remove | Y |  |  |  |  | Formatted |  | The list of features to remove when uninstalling a product from this set.  The default is "ALL".
Upgrade | UpgradeCode | N |  |  |  |  | Guid |  | The UpgradeCode GUID belonging to the products in this set.
Upgrade | VersionMax | Y |  |  |  |  | Text |  | The maximum ProductVersion of the products in this set.  The set may or may not include products with this particular version.
Upgrade | VersionMin | Y |  |  |  |  | Text |  | The minimum ProductVersion of the products in this set.  The set may or may not include products with this particular version.
'@
    foreach ($line in $validation -split "`n") {
        $line = $line.Trim()
        if (-not $line) { continue }
        $c = @($line -split ' \| ' | ForEach-Object { $_.Trim() })
        if ($c.Count -ne 10) { throw "_Validation row has $($c.Count) fields: $line" }
        # Built by index: a subexpression yielding nothing would vanish from an
        # array literal and shift every column after it.
        $row = [object[]]::new(10)
        foreach ($i in 0, 1, 2, 5, 7, 8, 9) { $row[$i] = $c[$i] }
        foreach ($i in 3, 4, 6) { if ($c[$i]) { $row[$i] = [int] $c[$i] } }
        try { Insert '_Validation' $row } catch { throw "_Validation row rejected: $line`n$($_.Exception.Message)" }
    }

    # Summary information. WordCount: 2 = compressed sources, 8 = elevated
    # privileges not required -- the bit that makes the package per-user.
    # Template names the platform, PageCount the schema, Revision is the
    # package code. The rest is what Explorer's Details tab shows a person
    # who right-clicks the file: the SDK's customary title, "Installation
    # Database", says what the file is to a tool and nothing to them, so
    # the title names the product and the subject says what it does. The
    # times are set because Explorer otherwise shows the file's creation
    # time, which NTFS tunnels from the build before when a file of the
    # same name was there seconds earlier.
    $si = Get-ComProperty $db 'SummaryInformation' @(20)
    $now = (Get-Date).ToUniversalTime()
    $summary = [ordered] @{
        2  = "$ProductName $Version installer"
        3  = 'Runs the executables you configure when a game starts and when it stops.'
        4  = $Author
        5  = "Installer; $ProductName; Windows; games"
        6  = "Built from commit $Commit. Documentation: $DocumentationUrl"
        7  = 'x64;1033'
        9  = $PackageCode
        12 = $now
        13 = $now
        14 = 500
        15 = 10
        18 = 'GameModeExecutor scripts\msi.ps1, over Windows Installer automation'
        19 = 0
    }
    # Enumerated, not indexed: an integer index into an ordered dictionary is
    # a position, not a key.
    foreach ($e in $summary.GetEnumerator()) { Set-ComProperty $si 'Property' @([int] $e.Key, $e.Value) }
    Invoke-Com $si 'Persist' @() | Out-Null
    Invoke-Com $db 'Commit' @() | Out-Null
    [System.Runtime.InteropServices.Marshal]::ReleaseComObject($si) | Out-Null
    [System.Runtime.InteropServices.Marshal]::ReleaseComObject($db) | Out-Null
    [System.Runtime.InteropServices.Marshal]::ReleaseComObject($installer) | Out-Null
} finally {
    # Whatever the runtime still holds on the package goes now, not at some
    # later collection: the file must be free when this script returns.
    [GC]::Collect(); [GC]::WaitForPendingFinalizers()
    Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
}

[pscustomobject] @{
    Path        = $Out
    ProductCode = $ProductCode
    UpgradeCode = $UpgradeCode
    PackageCode = $PackageCode
    Size        = (Get-Item $Out).Length
}
