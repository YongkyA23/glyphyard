<#
.SYNOPSIS
  Builds dist/Typeshelf-Setup-<version>.exe (apps/setup) with the server URL and install key baked in.
.DESCRIPTION
  -UpdateKey: the public key printed by `typeshelf-server update-keygen`. Without it the app never checks for
  updates and hides the "Update automatically" switch.
  Code signing: -CertThumbprint (a certificate in your store) or -CertFile (a .pfx; its password in the
  TYPESHELF_CERT_PASSWORD environment variable) signs typeshelf.exe and the setup program with signtool and a
  timestamp. Without either, the build is unsigned.
.EXAMPLE
  ./scripts/build-windows.ps1 -ServerUrl https://fonts.example.com -InstallKey (cargo run -q -p typeshelf-server -- install-key --data ./data)
#>
param(
    [Parameter(Mandatory)][string]$ServerUrl,
    [Parameter(Mandatory)][string]$InstallKey,
    [string]$UpdateKey = '',
    [string]$CertThumbprint = '',
    [string]$CertFile = '',
    [string]$TimestampUrl = 'http://timestamp.digicert.com'
)
$ErrorActionPreference = 'Stop'

$ServerUrl = $ServerUrl.Trim().TrimEnd('/')
if ($ServerUrl -notmatch '^https?://[^/\s]+') { throw "ServerUrl must start with http:// or https:// (got '$ServerUrl')." }
if ($InstallKey.Trim() -eq '') { throw 'InstallKey is empty. Get it with: typeshelf-server install-key --data <dir>' }

$root = Split-Path $PSScriptRoot -Parent

function Run([string]$dir, [string]$exe, [string[]]$arguments) {
    Push-Location $dir
    # Windows PowerShell 5 turns native stderr (progress output) into errors under 'Stop'; rely on exit codes.
    $ErrorActionPreference = 'Continue'
    try {
        & $exe @arguments
        if ($LASTEXITCODE -ne 0) { throw "$exe $arguments failed ($LASTEXITCODE)" }
    } finally { Pop-Location }
}

# option_env! in apps/desktop/src-tauri/src/sync.rs and apps/setup read these at compile time.
$env:TYPESHELF_SERVER_URL = $ServerUrl
$env:TYPESHELF_INSTALL_KEY = $InstallKey.Trim()
# apps/desktop/src-tauri/src/update.rs. Cleared when not given, so a key left in the shell is never baked in by accident.
$UpdateKey = $UpdateKey.Trim()
if ($UpdateKey -ne '' -and $UpdateKey -notmatch '^[0-9a-fA-F]{64}$') { throw 'UpdateKey must be the 64-character public key printed by typeshelf-server update-keygen.' }
if ($UpdateKey -ne '') { $env:TYPESHELF_UPDATE_KEY = $UpdateKey } else { Remove-Item Env:TYPESHELF_UPDATE_KEY -ErrorAction SilentlyContinue }

if ($CertThumbprint -ne '' -and $CertFile -ne '') { throw 'Give -CertThumbprint or -CertFile, not both.' }
$signArgs = $null
if ($CertThumbprint -ne '') { $signArgs = @('/sha1', $CertThumbprint.Replace(' ', '')) }
elseif ($CertFile -ne '') {
    if (-not $env:TYPESHELF_CERT_PASSWORD) { throw 'Set TYPESHELF_CERT_PASSWORD to the password of the .pfx file.' }
    $signArgs = @('/f', (Resolve-Path $CertFile).Path, '/p', $env:TYPESHELF_CERT_PASSWORD)
}
$signtool = $null
if ($signArgs) {
    $signtool = (Get-Command signtool.exe -ErrorAction SilentlyContinue).Source
    if (-not $signtool) {
        $signtool = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\signtool.exe" -ErrorAction SilentlyContinue | Sort-Object FullName | Select-Object -Last 1 -ExpandProperty FullName
    }
    if (-not $signtool) { throw 'signtool.exe not found: install the Windows SDK or put signtool on PATH.' }
} else {
    Write-Host 'Unsigned build: no certificate given (-CertThumbprint or -CertFile), so Windows SmartScreen may warn about the installer.'
}

function Sign([string]$file) {
    if (-not $signArgs) { return }
    Run $root $signtool (@('sign', '/fd', 'SHA256', '/tr', $TimestampUrl, '/td', 'SHA256') + $signArgs + @($file))
}

# 1. The app: tauri.conf.json's beforeBuildCommand builds the UI and the plug-in packages (plugins/dist).
Run "$root/apps/desktop/ui" 'bun' @('install', '--frozen-lockfile')
Run "$root/apps/desktop/src-tauri" 'bunx' @('@tauri-apps/cli', 'build', '--no-bundle')

# 2. The payload: typeshelf.exe + plugins/**. tar writes zip entries with '/' separators.
$version = (cargo pkgid --manifest-path "$root/Cargo.toml" -p typeshelf-desktop) -replace '.*[#@]', ''
$stage = "$root/target/setup-payload"
$payload = "$root/target/typeshelf-payload.zip"
Remove-Item $stage, $payload -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force "$stage/plugins" | Out-Null
Sign "$root/target/release/typeshelf.exe"
Copy-Item "$root/target/release/typeshelf.exe" $stage
Copy-Item "$root/plugins/dist/*" "$stage/plugins" -Recurse
Run $stage 'tar.exe' @('-a', '-c', '-f', $payload, 'typeshelf.exe', 'plugins')

# 3. The setup program, with the payload embedded.
$env:TYPESHELF_PAYLOAD = (Resolve-Path $payload).Path
$env:TYPESHELF_VERSION = $version
Run $root 'cargo' @('build', '-p', 'typeshelf-setup', '--release')

New-Item -ItemType Directory -Force "$root/dist" | Out-Null
$out = "$root/dist/Typeshelf-Setup-$version.exe"
Copy-Item "$root/target/release/typeshelf-setup.exe" $out -Force
Sign $out
$updates = if ($UpdateKey -ne '') { 'on' } else { 'off (no -UpdateKey)' }
Write-Host "Installer: $((Resolve-Path $out).Path)  (version $version, server: $ServerUrl, updates: $updates)"
if ($UpdateKey -ne '') { Write-Host "To publish it: typeshelf-server update-sign `"$out`" --key <private key file>, then upload all three files in admin Settings." }
