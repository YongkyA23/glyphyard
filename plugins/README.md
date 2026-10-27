# Adobe auto-activation plug-ins

When a document opens in Photoshop, Illustrator or InDesign, the plug-in sends the document's fonts to the
Typeshelf app, which turns on the ones in its library (and turns them off again when the last document that
needed them closes).

| Folder | App | Tech | Min version |
|---|---|---|---|
| `photoshop-uxp/` | Photoshop | UXP, manifest v5 | 23.3 (2022) |
| `indesign-uxp/` | InDesign | UXP, manifest v5 | 18.5 (2023) |
| `illustrator-cep/` | Illustrator | CEP 11 panel + ExtendScript | 26 (2022) |
| `shared/` | all | `typeshelf-client.js` (bridge client + panel logic), `index.html` (panel) | |

`build.ts` copies `shared/` into each plug-in. Each `main.js` only knows how to list documents, read their
fonts and listen for document events; everything else lives in `shared/typeshelf-client.js`.

## Talking to the app

The app runs `crates/adobe-bridge`: HTTP on `127.0.0.1` only, port 27716 (or the next free one up to 27720;
the chosen port is also written to `adobe-bridge.json` in the app data dir). Plug-ins probe 27716–27720.

| Request | Body | Answer |
|---|---|---|
| `GET /v1/ping` (token optional) | | `{app: "typeshelf", version, paired}` |
| `POST /v1/pair` | `{app, appVersion, clientId}` | `{token}` once the user allows it in Typeshelf (waits ≤ 60 s); 403 if declined or no answer; 409 while another prompt is open |
| `POST /v1/document` (token required) | `{app, document, action: "open" \| "close", fonts: [{postscriptName, family, style, missing}]}` | open → `{activated, missing, alreadyActive}`; close → `{released}`; 401 without a valid token |

Every request must send `X-Typeshelf-Client` and `Host: 127.0.0.1:<port>` (or `localhost`). The bridge
sends no CORS headers and answers `OPTIONS` with 403, so a web page can't call it: a browser has to
preflight the custom header and the preflight fails. The Host check stops DNS-rebinding pages, and
non-loopback peers are refused. Tokens are stored hashed (`adobe-tokens.json`); the app can revoke them per
app. That is why the Illustrator panel uses Node's `http` module instead of `fetch`: in CEF, `fetch` would
preflight too.

## Events per app

| App | Document opened | Document closed | Already-open documents | Fonts | Missing fonts |
|---|---|---|---|---|---|
| Photoshop | `action.addNotificationListener(["open"])` | `"close"` notification | on load | `batchPlay` `get textKey` of every text layer (groups included): `textStyleRange[].textStyle.fontPostScriptName / fontName / fontStyleName` | PostScript name not in `app.fonts` |
| InDesign | `afterOpen` | `afterClose` | on load (+ `afterActivate`) | `document.fonts` (`postscriptName`, `fontFamily`, `fontStyleName`) | `font.status !== FontStatus.INSTALLED` |
| Illustrator | `documentAfterOpen` / `documentAfterActivate` | detected on `documentAfterDeactivate` / `documentAfterActivate` / `applicationActivate` (no close event) | on load (the panel starts with the app via `StartOn`) | XMP font list (`xmpTPg:Fonts`, includes fonts that are missing now) + `textFrames[].textRanges[].characterAttributes.textFont` | `app.textFonts.getByName()` throws |

Events only trigger a re-sync: the panel diffs the open documents against what it already told Typeshelf,
sends `open` for new ones and `close` for vanished ones. "Check this document" re-sends the active document.
While Typeshelf isn't running the panel retries every 15 s.

## Build and check

```sh
bun plugins/build.ts               # → plugins/dist/typeshelf-photoshop.ccx, typeshelf-indesign.ccx, com.typeshelf.illustrator/
bunx -p typescript tsc -p plugins  # type-checks the plug-in JS (JSDoc + checkJs)
cargo run -p typeshelf-adobe-bridge --example fake_host   # bridge without the desktop app: approves every pairing
```

A `.ccx` is a zip of the plug-in folder with `manifest.json` at its root. If Creative Cloud refuses it,
package the staged folder with the UXP Developer Tool ("Package") instead.

## Installing by hand

### Photoshop / InDesign (UXP `.ccx`)

Double-click the `.ccx` (Creative Cloud desktop installs it), or run the Unified Plugin Installer Agent:

- Windows:
  `"C:\Program Files\Common Files\Adobe\Adobe Desktop Common\RemoteComponents\UPI\UnifiedPluginInstallerAgent\UnifiedPluginInstallerAgent.exe" /install "C:\path\to\typeshelf-photoshop.ccx"`
- macOS:
  `"/Library/Application Support/Adobe/Adobe Desktop Common/RemoteComponents/UPI/UnifiedPluginInstallerAgent/UnifiedPluginInstallerAgent.app/Contents/MacOS/UnifiedPluginInstallerAgent" --install /path/to/typeshelf-photoshop.ccx`

Restart the app, then open Plugins → Typeshelf and press "Pair with Typeshelf".

### Illustrator (CEP)

Copy `dist/com.typeshelf.illustrator/` into the per-user CEP extensions folder:

- Windows: `%APPDATA%\Adobe\CEP\extensions\com.typeshelf.illustrator`
- macOS: `~/Library/Application Support/Adobe/CEP/extensions/com.typeshelf.illustrator`

The extension is unsigned, so allow unsigned extensions for the CEP runtime the app uses:

| CSXS | Illustrator | Windows (registry, string value) | macOS |
|---|---|---|---|
| 11 | 2022–2024 (26–28) | `HKCU\Software\Adobe\CSXS.11` `PlayerDebugMode` = `"1"` | `defaults write com.adobe.CSXS.11 PlayerDebugMode 1` |
| 12 | 2025+ (29+) | `HKCU\Software\Adobe\CSXS.12` `PlayerDebugMode` = `"1"` | `defaults write com.adobe.CSXS.12 PlayerDebugMode 1` |

Windows one-liner: `reg add HKCU\Software\Adobe\CSXS.11 /v PlayerDebugMode /t REG_SZ /d 1 /f`.
Restart Illustrator; the panel is under Window → Extensions → Typeshelf.

`PlayerDebugMode` lets *any* unsigned CEP extension load. For releases, sign instead and skip it:

```sh
ZXPSignCmd -selfSignedCert US CA Typeshelf Typeshelf <password> typeshelf.p12   # or a real code-signing cert
ZXPSignCmd -sign plugins/dist/com.typeshelf.illustrator typeshelf-illustrator.zxp typeshelf.p12 <password> -tsa http://timestamp.digicert.com
```

A `.zxp` installs with the same Unified Plugin Installer Agent command as a `.ccx`.

## What the desktop app runs (`adobe_install` / `adobe_remove`)

Implemented in `crates/adobe-bridge/src/install.rs` (`detect_apps()`, `install(app, dist_dir)`,
`remove(app)`); `dist_dir` is where the app bundles `plugins/dist/`. "UPIA" is the agent path above.

| | Windows | macOS |
|---|---|---|
| Detect app versions | folders `%ProgramFiles%\Adobe\Adobe {Photoshop,Illustrator,InDesign} <year>` | folders `/Applications/Adobe {Photoshop,Illustrator,InDesign} <year>` |
| Photoshop/InDesign installed? | `UPIA /list all` lists "Typeshelf for Photoshop" / "Typeshelf for InDesign" | `UPIA --list all` (same names) |
| Install Photoshop/InDesign | `UPIA /install <dist>\typeshelf-<app>.ccx`, then `/list all` to confirm | `UPIA --install <dist>/typeshelf-<app>.ccx`, then `--list all` |
| Remove Photoshop/InDesign | `UPIA /remove "Typeshelf for Photoshop"` (or InDesign) | `UPIA --remove "Typeshelf for Photoshop"` |
| Illustrator installed? | `%APPDATA%\Adobe\CEP\extensions\com.typeshelf.illustrator\CSXS\manifest.xml` exists | `~/Library/Application Support/Adobe/CEP/extensions/com.typeshelf.illustrator/CSXS/manifest.xml` exists |
| Install Illustrator | replace that folder with `<dist>\com.typeshelf.illustrator`; set `PlayerDebugMode` = `"1"` under `HKCU\Software\Adobe\CSXS.11` and `.12` | replace that folder; `defaults write com.adobe.CSXS.{11,12} PlayerDebugMode 1` |
| Remove Illustrator | delete the folder (`PlayerDebugMode` is left alone: other extensions may need it) | same |

The Adobe app must be restarted after install/remove. UPIA ships with Creative Cloud desktop; without it,
`install`/`remove` for Photoshop/InDesign fail with "Creative Cloud's plug-in installer was not found".
