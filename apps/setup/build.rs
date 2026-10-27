use std::path::PathBuf;

fn main() {
    // Real builds pass the payload zip; dev builds (cargo build / clippy) get an empty archive.
    println!("cargo:rerun-if-env-changed=TYPESHELF_PAYLOAD");
    let payload = match std::env::var("TYPESHELF_PAYLOAD") {
        Ok(p) if !p.is_empty() => PathBuf::from(p),
        _ => {
            let p = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("empty-payload.zip");
            // An empty zip is just the end-of-central-directory record.
            let mut eocd = vec![0x50, 0x4b, 0x05, 0x06];
            eocd.resize(22, 0);
            std::fs::write(&p, eocd).unwrap();
            p
        }
    };
    println!("cargo:rerun-if-changed={}", payload.display());
    println!("cargo:rustc-env=TYPESHELF_PAYLOAD={}", payload.display());

    println!("cargo:rerun-if-env-changed=TYPESHELF_VERSION");
    let version = std::env::var("TYPESHELF_VERSION")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| std::env::var("CARGO_PKG_VERSION").unwrap());
    println!("cargo:rustc-env=TYPESHELF_VERSION={version}");
    println!("cargo:rerun-if-env-changed=TYPESHELF_SERVER_URL");

    let windows = tauri_build::WindowsAttributes::new().app_manifest(include_str!("setup.manifest"));
    tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows)).expect("tauri build");
}
