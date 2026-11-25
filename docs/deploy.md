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
