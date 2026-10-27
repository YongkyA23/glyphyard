<#
.SYNOPSIS
  Builds the Windows installer (NSIS) with the server URL and install key baked in.
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

# option_env! in apps/desktop/src-tauri/src/sync.rs reads these at compile time.
$env:TYPESHELF_SERVER_URL = $ServerUrl
$env:TYPESHELF_INSTALL_KEY = $InstallKey.Trim()

# tauri.conf.json's beforeBuildCommand builds the UI and the plug-in packages (bundled as resources).
Run "$root/apps/desktop/ui" 'bun' @('install', '--frozen-lockfile')
Run "$root/apps/desktop/src-tauri" 'bunx' @('@tauri-apps/cli', 'build', '--bundles', 'nsis')

$out = Get-ChildItem "$root/target/release/bundle/nsis/*-setup.exe" | Sort-Object LastWriteTime | Select-Object -Last 1
New-Item -ItemType Directory -Force "$root/dist" | Out-Null
Copy-Item $out.FullName "$root/dist/" -Force
Write-Host "Installer: $root\dist\$($out.Name)  (server: $ServerUrl)"
