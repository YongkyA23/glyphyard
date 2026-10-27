# Deploying the server (qrvm → https://fonts.collabproject.web.id)

```
Internet ─HTTPS─▶ Cloudflare (proxied A record) ─▶ vpnwg 103.150.196.24 nginx + Let's Encrypt
        ─WireGuard wg0─▶ MikroTik 10.0.0.2:8095 (dst-nat "wgs_fonts_8095") ─▶ qrvm 192.168.8.102:8095
```
Same path as the other collabproject.web.id sites: the MikroTik has a dst-nat rule like the other wgs_* sites,
plus a static DHCP lease so qrvm keeps 192.168.8.102.

## qrvm (user `qr`)
```
~/typeshelf/bin/typeshelf-server    Linux binary (cross-compiled on Windows, see below)
~/typeshelf/static/                 admin UI (apps/admin-ui build)
~/typeshelf/data/                   SQLite DB + shared library files (back this up)
~/.config/systemd/user/typeshelf.service   serve --host 192.168.8.102 --port 8095 --public-url https://fonts.collabproject.web.id (user service, no sudo)
```
It runs as a systemd **user** service of `qr`; `loginctl enable-linger qr` makes it start at boot without a login.
```
systemctl --user status typeshelf        # restart / stop / journalctl --user -u typeshelf
~/typeshelf/bin/typeshelf-server create-admin you@example.com --data ~/typeshelf/data   # prompts for a password
```

## vpnwg
- `/etc/nginx/sites-available/fonts.collabproject.web.id` (active: 80→443 redirect, TLS, WebSocket for
  `/api/v1/me/ws`, 512 MB uploads, `proxy_pass http://10.0.0.2:8095`), plus the same file with `.https`.
- Certificate: Let's Encrypt via certbot webroot (`~/enable-fonts-https.sh`), renewed by certbot's timer.

## Cloudflare DNS
`fonts` · A · `103.150.196.24` · Proxied (like jobs/portal/qr).

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
cd apps/admin-ui; bun run build   # → apps/server/static
```
Upload the binary to `~/typeshelf/bin/` and `apps/server/static/*` to `~/typeshelf/static/`, then
`systemctl --user restart typeshelf`.

## Installer for this server
```
ssh qrvm '~/typeshelf/bin/typeshelf-server install-key --data ~/typeshelf/data'
./scripts/build-windows.ps1 -ServerUrl https://fonts.collabproject.web.id -InstallKey <key>
```
