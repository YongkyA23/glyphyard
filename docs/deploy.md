# Deploying the server (example: <server-host> → https://fonts.example.com)

Placeholders below stand for your own values. The real ones for this deployment are in `docs/deploy.local.md`
(not committed).

```
Internet ─HTTPS─▶ Cloudflare (proxied A record) ─▶ <proxy-host> <origin-ip> nginx + Let's Encrypt
        ─WireGuard wg0─▶ router <tunnel-ip>:8095 (dst-nat "wgs_fonts_8095") ─▶ <server-host> <server-lan-ip>:8095
```
Same path as your other sites behind the proxy: the router has a dst-nat rule like the other wgs_* sites,
plus a static DHCP lease so the server keeps `<server-lan-ip>`.

## <server-host> (user `<server-user>`)
```
~/typeshelf/bin/typeshelf-server    Linux binary (cross-compiled on Windows, see below)
~/typeshelf/static/                 admin UI (apps/admin-ui build)
~/typeshelf/data/                   SQLite DB + shared library files (back this up)
~/.config/systemd/user/typeshelf.service   serve --host <server-lan-ip> --port 8095 --public-url https://fonts.example.com (user service, no sudo)
```
It runs as a systemd **user** service of `<server-user>`; `loginctl enable-linger <server-user>` makes it start at boot without a login.
Open the admin UI through the https public URL (the proxy must pass `X-Forwarded-Proto` or the original `Host` for the session cookie to be marked `Secure`); `http://<server-lan-ip>:8095` also works, with a non-`Secure` cookie, but sends the password unencrypted over the LAN.
```
systemctl --user status typeshelf        # restart / stop / journalctl --user -u typeshelf
~/typeshelf/bin/typeshelf-server create-admin you@example.com --data ~/typeshelf/data   # prompts for a password
```

## <proxy-host>
- `/etc/nginx/sites-available/fonts.example.com` (active: 80→443 redirect, TLS, WebSocket for
  `/api/v1/me/ws`, 512 MB uploads, `proxy_pass http://<tunnel-ip>:8095`), plus the same file with `.https`.
- Certificate: Let's Encrypt via certbot webroot (`~/enable-fonts-https.sh`), renewed by certbot's timer.

## Cloudflare DNS
`fonts` · A · `<origin-ip>` · Proxied.

## Router (MikroTik)
```
/ip firewall nat add chain=dstnat in-interface=wireguard-client dst-address=<tunnel-ip> protocol=tcp dst-port=8095 action=dst-nat to-addresses=<server-lan-ip> to-ports=8095 comment="wgs_fonts_8095"
/ip dhcp-server lease make-static [find where mac-address="<server-mac>" dynamic]
/ip dhcp-server lease set [find where mac-address="<server-mac>"] address=<server-lan-ip> comment="Typeshelf server"
```

## Updating the server
Build on Windows:
```
pip install --user ziglang            # zig.exe from this package must be on PATH
cargo install cargo-zigbuild --locked
rustup target add x86_64-unknown-linux-gnu
$env:CARGO_TARGET_DIR = "target-linux"
cargo zigbuild -p typeshelf-server --release --target x86_64-unknown-linux-gnu.2.35
cd apps/admin-ui; bun run build   # → apps/server/static
```
Upload the binary to `~/typeshelf/bin/` and `apps/server/static/*` to `~/typeshelf/static/`, then
`systemctl --user restart typeshelf`.

## Installer for this server
```
ssh <server-host> '~/typeshelf/bin/typeshelf-server install-key --data ~/typeshelf/data'
./scripts/build-windows.ps1 -ServerUrl https://fonts.example.com -InstallKey <key>
```

Without `-ServerUrl` (and `-InstallKey`) the script makes the standalone build, with no server baked in; a server
can be added later in the app's Settings › Connection:
```
./scripts/build-windows.ps1
```

Optional, both off by default:
- **Code signing**: add `-CertThumbprint <thumbprint>` (a certificate in your Windows store) or `-CertFile <file.pfx>`
  with the password in `$env:TYPESHELF_CERT_PASSWORD`. `signtool` (Windows SDK) signs `typeshelf.exe` and the
  setup program with an RFC 3161 timestamp (`-TimestampUrl`, default DigiCert). Without a certificate the script
  prints one line saying the build is unsigned.
- **App updates from this server**: once, on a computer you keep offline,
  `typeshelf-server update-keygen --out update-key.txt` writes the private key (owner-only file on Linux/macOS) and prints the public key. Build with
  `-UpdateKey <public key>`; a build without it never checks for updates and hides Settings › "Update automatically".
  For each release: `typeshelf-server update-sign dist/Glyphyard-Setup-<version>.exe --key update-key.txt` writes
  `<setup>.manifest` and `<setup>.manifest.sig`; upload the three files in admin Settings › App updates (the
  manifest says whether the release is for Windows or Mac; each has its own slot). Apps
  check a minute after start and daily, verify the signature and the file, ignore versions that are not newer,
  and run the setup program with `--silent` when the user quits (not when Glyphyard Setup or the uninstaller is the one closing the app). A release the app refuses shows a notice in its Settings under "Update automatically"; it usually means the release was signed with a different key than the app was built with. Anyone with the private key can ship code to
  every computer: keep it off the server. Losing it means rebuilding the app with a new key.

## Mac release
Build on a Mac with Xcode command line tools (`xcode-select --install`), bun, and both Rust targets:
```
rustup target add aarch64-apple-darwin x86_64-apple-darwin
./scripts/build-macos.sh --server-url https://fonts.example.com --install-key <key> --update-key <public key>
```
It writes `dist/Glyphyard-<version>.dmg` (a universal app) and `dist/Glyphyard-<version>.app.tar.gz` (what updates
install). The server URL, install key and update public key are the same ones as the Windows build; the
private key stays on the signing PC. Carry the `.app.tar.gz` there, run
`typeshelf-server update-sign dist/Glyphyard-<version>.app.tar.gz --key update-key.txt`, and upload the three
files in admin Settings › App updates.

Without `--sign-identity` the app is ad-hoc signed and the script prints one line saying so. To sign, add
`--sign-identity "Developer ID Application: <name> (<team id>)"`; to notarize as well, set either
`APPLE_API_KEY`, `APPLE_API_ISSUER` and `APPLE_API_KEY_PATH`, or `APPLE_ID`, `APPLE_PASSWORD` and `APPLE_TEAM_ID`.

An unsigned build:
- is blocked on first open until allowed in System Settings › Privacy & Security › Open Anyway;
- may need Glyphyard allowed once under App Management for updates to apply;
- may ask for folder access again after an update.

Install the app in `/Applications` (or a folder the user can write to), or updates cannot apply. An update
replaces the app when the user quits; Glyphyard does not relaunch, the new version starts the next time it is opened.
