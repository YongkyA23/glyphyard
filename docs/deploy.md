# Deploying the server (qrvm → https://fonts.collabproject.web.id)

Layout on the host (`qr` user):
```
~/typeshelf/bin/typeshelf-server    Linux binary (cross-compiled on Windows, see below)
~/typeshelf/static/                 admin UI (apps/admin-ui build)
~/typeshelf/data/                   SQLite DB + shared library files (back this up)
~/typeshelf/deploy/typeshelf.service
```
The server listens on `127.0.0.1:8095` only. Public HTTPS comes from a Cloudflare Tunnel
(`cloudflared` → `http://localhost:8095`), so no router port or nginx change is needed.

## Build (on Windows)
```
pip install --user ziglang
cargo install cargo-zigbuild --locked
rustup target add x86_64-unknown-linux-gnu
# zig.exe from the ziglang package must be on PATH
$env:CARGO_TARGET_DIR = "target-linux"
cargo zigbuild -p typeshelf-server --release --target x86_64-unknown-linux-gnu.2.35
cd apps/admin-ui; bun run build   # → apps/server/static
```
Upload `target-linux/x86_64-unknown-linux-gnu/release/typeshelf-server` to `~/typeshelf/bin/` and the contents of
`apps/server/static` to `~/typeshelf/static/`, then `sudo systemctl restart typeshelf`.

## One-time setup (needs sudo, run on the host)
```
sudo cp ~/typeshelf/deploy/typeshelf.service /etc/systemd/system/
sudo systemctl daemon-reload && sudo systemctl enable --now typeshelf
~/typeshelf/bin/typeshelf-server create-admin you@example.com --data ~/typeshelf/data
```
Cloudflare Tunnel: Cloudflare dashboard → Zero Trust → Networks → Tunnels → Create (cloudflared) → copy the
install command shown for Debian/Ubuntu (it contains your token) and run it on the host → Public hostname:
`fonts.collabproject.web.id`, service `HTTP` → `localhost:8095`.

## Installer for this server
```
ssh qrvm '~/typeshelf/bin/typeshelf-server install-key --data ~/typeshelf/data'
./scripts/build-windows.ps1 -ServerUrl https://fonts.collabproject.web.id -InstallKey <key>
```
