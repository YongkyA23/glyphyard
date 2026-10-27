# Deploying the server (qrvm â†’ https://fonts.collabproject.web.id)

```
Internet â”€HTTPSâ”€â–¶ Cloudflare (proxied A record) â”€â–¶ vpnwg 103.150.196.24 nginx + Let's Encrypt
        â”€WireGuard wg0â”€â–¶ MikroTik 10.0.0.2 â”€LANâ”€â–¶ qrvm 192.168.8.102:8095 (typeshelf-server)
```
Same path as the other collabproject.web.id sites. vpnwg already routes 192.168.8.0/24 over wg0, so the MikroTik
has a dst-nat rule like the other wgs_* sites, plus a static DHCP lease so qrvm keeps 192.168.8.102.

## qrvm (user `qr`)
```
~/typeshelf/bin/typeshelf-server    Linux binary (cross-compiled on Windows, see below)
~/typeshelf/static/                 admin UI (apps/admin-ui build)
~/typeshelf/data/                   SQLite DB + shared library files (back this up)
~/.config/systemd/user/typeshelf.service   serve --host 192.168.8.102 --port 8095 (user service, no sudo)
```
It runs as a systemd **user** service of `qr`; `loginctl enable-linger qr` makes it start at boot without a login.
```
systemctl --user status typeshelf        # restart / stop / journalctl --user -u typeshelf
~/typeshelf/bin/typeshelf-server create-admin you@example.com --data ~/typeshelf/data   # prompts for a password
```

## vpnwg
- `/etc/nginx/sites-available/fonts.collabproject.web.id` (HTTP until the certificate exists) and
  `fonts.collabproject.web.id.https` (final: 80â†’443 redirect, TLS, WebSocket for `/api/v1/me/ws`, 512 MB uploads).
- After the DNS record exists: `~/enable-fonts-https.sh` (certbot webroot, swaps in the HTTPS config, reloads nginx).

## Cloudflare DNS
`fonts` Â· A Â· `103.150.196.24` Â· Proxied (like jobs/portal/qr).

## MikroTik
```
/ip firewall nat add chain=dstnat in-interface=wireguard-client dst-address=10.0.0.2 protocol=tcp dst-port=8095 action=dst-nat to-addresses=192.168.8.102 to-ports=8095 comment="wgs_fonts_8095"
/ip dhcp-server lease make-static [find where mac-address="BC:24:11:CF:C9:00" dynamic]
/ip dhcp-server lease set [find where mac-address="BC:24:11:CF:C9:00"] address=192.168.8.102 comment="qrvm (Typeshelf server)"
```

## Updating the server
Build on Windows:
```
pip install --user ziglang            # zig.exe from this package must be on PATH
cargo install cargo-zigbuild --locked
rustup target add x86_64-unknown-linux-gnu
$env:CARGO_TARGET_DIR = "target-linux"
cargo zigbuild -p typeshelf-server --release --target x86_64-unknown-linux-gnu.2.35
cd apps/admin-ui; bun run build   # â†’ apps/server/static
```
Upload the binary to `~/typeshelf/bin/` and `apps/server/static/*` to `~/typeshelf/static/`, then
`systemctl --user restart typeshelf`.

## Installer for this server
```
ssh qrvm '~/typeshelf/bin/typeshelf-server install-key --data ~/typeshelf/data'
./scripts/build-windows.ps1 -ServerUrl https://fonts.collabproject.web.id -InstallKey <key>
```
