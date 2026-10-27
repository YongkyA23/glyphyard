<#
.SYNOPSIS
  Builds dist/Typeshelf-Setup-<version>.exe (apps/setup) with the server URL and install key baked in.
.EXAMPLE
  ./scripts/build-windows.ps1 -ServerUrl https://fonts.example.com -InstallKey (cargo run -q -p typeshelf-server -- install-key --data ./data)
#>
param(
    [Parameter(Mandatory)][string]$ServerUrl,
    [Parameter(Mandatory)][string]$InstallKey
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

# 1. The app: tauri.conf.json's beforeBuildCommand builds the UI and the plug-in packages (plugins/dist).
Run "$root/apps/desktop/ui" 'bun' @('install', '--frozen-lockfile')
Run "$root/apps/desktop/src-tauri" 'bunx' @('@tauri-apps/cli', 'build', '--no-bundle')

# 2. The payload: typeshelf.exe + plugins/**. tar writes zip entries with '/' separators.
$version = (cargo pkgid --manifest-path "$root/Cargo.toml" -p typeshelf-desktop) -replace '.*[#@]', ''
$stage = "$root/target/setup-payload"
$payload = "$root/target/typeshelf-payload.zip"
Remove-Item $stage, $payload -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force "$stage/plugins" | Out-Null
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
Write-Host "Installer: $((Resolve-Path $out).Path)  (version $version, server: $ServerUrl)"
