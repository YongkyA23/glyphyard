#!/usr/bin/env bash
# Builds dist/Glyphyard-<version>.dmg and dist/Glyphyard-<version>.app.tar.gz (universal Mac app) with the
# server URL and install key baked in. Run on a Mac.
#
# Usage: scripts/build-macos.sh [--server-url URL] [--install-key KEY] [--update-key HEX]
#                               [--sign-identity "Developer ID Application: ..."]
#
#   --update-key     the public key printed by `typeshelf-server update-keygen`. Without it the app never checks
#                    for updates and hides the "Update automatically" switch.
#   --sign-identity  signs the app with codesign. Notarization runs too when Tauri's Apple variables are set:
#                    APPLE_API_KEY + APPLE_API_ISSUER + APPLE_API_KEY_PATH, or APPLE_ID + APPLE_PASSWORD +
#                    APPLE_TEAM_ID. Without --sign-identity the build is ad-hoc signed.
#   Without --server-url it makes the standalone build: no server baked in (one can be added later in the
#   app's Settings > Connection).
#
# Example: ./scripts/build-macos.sh --server-url https://fonts.example.com --install-key <key>
set -euo pipefail

server_url='' install_key='' update_key='' sign_identity=''
usage() { sed -n '2,/^# Example/p' "$0" | sed 's/^# \{0,1\}//'; }
while [ $# -gt 0 ]; do
    case "$1" in
        --server-url|--install-key|--update-key|--sign-identity)
            [ $# -ge 2 ] || { echo "$1 needs a value." >&2; exit 1; }
            case "$1" in
                --server-url) server_url=$2 ;;
                --install-key) install_key=$2 ;;
                --update-key) update_key=$2 ;;
                --sign-identity) sign_identity=$2 ;;
            esac
            shift 2 ;;
        -h|--help) usage; exit 0 ;;
        *) echo "Unknown argument: $1" >&2; usage >&2; exit 1 ;;
    esac
done

trim() { local s=$1; s="${s#"${s%%[![:space:]]*}"}"; printf '%s' "${s%"${s##*[![:space:]]}"}"; }
server_url=$(trim "$server_url"); server_url=${server_url%/}
install_key=$(trim "$install_key"); update_key=$(trim "$update_key"); sign_identity=$(trim "$sign_identity")

if [ -z "$server_url" ]; then
    [ -z "$install_key" ] || { echo 'An install key needs a server: give --server-url too, or neither for the standalone build.' >&2; exit 1; }
else
    [[ "$server_url" =~ ^https?://[^/[:space:]]+ ]] || { echo "--server-url must start with http:// or https:// (got '$server_url')." >&2; exit 1; }
    [ -n "$install_key" ] || { echo 'The install key is empty. Get it with: typeshelf-server install-key --data <dir>' >&2; exit 1; }
fi
if [ -n "$update_key" ] && ! [[ "$update_key" =~ ^[0-9a-fA-F]{64}$ ]]; then
    echo 'The update key must be the 64-character public key printed by typeshelf-server update-keygen.' >&2; exit 1
fi

[ "$(uname)" = Darwin ] || { echo 'build-macos.sh must run on a Mac.' >&2; exit 1; }
command -v bun >/dev/null || { echo 'bun not found: install it from https://bun.sh' >&2; exit 1; }
xcode-select -p >/dev/null 2>&1 || { echo 'Xcode command line tools not found: run xcode-select --install' >&2; exit 1; }
installed=$(rustup target list --installed)
for t in aarch64-apple-darwin x86_64-apple-darwin; do
    if ! grep -qx "$t" <<<"$installed"; then
        echo 'Missing Rust targets. Run: rustup target add aarch64-apple-darwin x86_64-apple-darwin' >&2; exit 1
    fi
done

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

# option_env! in apps/desktop/src-tauri reads these at compile time. Unset when not given, so a value left in
# the shell is never baked in.
if [ -n "$server_url" ]; then
    export TYPESHELF_SERVER_URL=$server_url TYPESHELF_INSTALL_KEY=$install_key
else
    unset TYPESHELF_SERVER_URL TYPESHELF_INSTALL_KEY
    echo 'Standalone build: no server baked in.'
fi
if [ -n "$update_key" ]; then export TYPESHELF_UPDATE_KEY=$update_key; else unset TYPESHELF_UPDATE_KEY; fi

notarize=''
config='{"productName":"Glyphyard"}'
if [ -n "$sign_identity" ]; then
    export APPLE_SIGNING_IDENTITY=$sign_identity
    if [ -n "${APPLE_API_KEY:-}" ] && [ -n "${APPLE_API_ISSUER:-}" ] && [ -n "${APPLE_API_KEY_PATH:-}" ]; then
        notarize=api
    elif [ -n "${APPLE_ID:-}" ] && [ -n "${APPLE_PASSWORD:-}" ] && [ -n "${APPLE_TEAM_ID:-}" ]; then
        notarize=apple-id
    fi
else
    unset APPLE_SIGNING_IDENTITY APPLE_API_KEY APPLE_API_ISSUER APPLE_API_KEY_PATH APPLE_ID APPLE_PASSWORD APPLE_TEAM_ID
    config='{"productName":"Glyphyard","bundle":{"macOS":{"signingIdentity":"-"}}}'
    echo 'Unsigned build: no --sign-identity given, so the DMG is ad-hoc signed and Gatekeeper will warn on first open.'
fi

# 1. The app: tauri.conf.json's beforeBuildCommand builds the UI and the plug-in packages (plugins/dist).
(cd apps/desktop/ui && bun install --frozen-lockfile)
(cd apps/desktop/src-tauri && bunx @tauri-apps/cli build --target universal-apple-darwin --bundles app,dmg --config "$config")

# 2. The outputs.
version=$(cargo pkgid --manifest-path Cargo.toml -p typeshelf-desktop | sed 's/.*[#@]//')
bundle=target/universal-apple-darwin/release/bundle
mkdir -p dist
dmg=dist/Glyphyard-$version.dmg
cp "$bundle"/dmg/Glyphyard_*_universal.dmg "$dmg"

# Never echo these command lines: they can hold the Apple password.
if [ "$notarize" = api ]; then
    xcrun notarytool submit "$dmg" --wait --key "$APPLE_API_KEY_PATH" --key-id "$APPLE_API_KEY" --issuer "$APPLE_API_ISSUER" >/dev/null \
        || { echo "Notarization of $dmg failed." >&2; exit 1; }
elif [ "$notarize" = apple-id ]; then
    xcrun notarytool submit "$dmg" --wait --apple-id "$APPLE_ID" --password "$APPLE_PASSWORD" --team-id "$APPLE_TEAM_ID" >/dev/null \
        || { echo "Notarization of $dmg failed." >&2; exit 1; }
fi
if [ -n "$notarize" ]; then
    xcrun stapler staple "$dmg"
fi

# The updater's payload: the app bundle alone, without AppleDouble (._*) entries.
tarball=dist/Glyphyard-$version.app.tar.gz
COPYFILE_DISABLE=1 tar -czf "$tarball" -C "$bundle/macos" Glyphyard.app

if [ -n "$update_key" ]; then updates=on; else updates='off (no --update-key)'; fi
server=${server_url:-'none (standalone)'}
signed=${sign_identity:-ad-hoc}
echo "DMG: $root/$dmg  (version $version, server: $server, updates: $updates, signed: $signed)"
if [ -n "$update_key" ]; then
    echo "To publish the update: typeshelf-server update-sign \"$tarball\" --key <private key file> (on the computer that keeps it), then upload the three files in admin Settings › App updates."
fi
