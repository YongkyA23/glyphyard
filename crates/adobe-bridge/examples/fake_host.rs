//! Stand-in for the desktop app when testing the Adobe plug-ins:
//! `cargo run -p typeshelf-adobe-bridge --example fake_host [data_dir]`.
//! Approves every pairing and "activates" any font whose PostScript name starts with "Lib-" or is in
//! the TYPESHELF_FAKE_LIBRARY env var (comma-separated).

use std::sync::Arc;

use async_trait::async_trait;
use typeshelf_adobe_bridge::*;

struct FakeHost(Vec<String>);

#[async_trait]
impl BridgeHandler for FakeHost {
    async fn approve_pairing(&self, req: PairRequest) -> bool {
        println!("pairing {:?} {} ({}) -> allowed", req.app, req.app_version, req.client_id);
        true
    }

    async fn activate(&self, req: DocRequest) -> DocResult {
        let mut r = DocResult::default();
        for f in req.fonts {
            let name = f.postscript_name;
            if name.starts_with("Lib-") || self.0.contains(&name) {
                r.activated.push(name);
            } else if f.missing {
                r.missing.push(name);
            } else {
                r.already_active.push(name);
            }
        }
        println!("{:?} opened {}: {r:?}", req.app, req.document);
        r
    }

    async fn release(&self, names: Vec<String>) {
        println!("release {names:?}");
    }
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let data_dir = std::env::args().nth(1).unwrap_or_else(|| "target/fake-host".into());
    let library = std::env::var("TYPESHELF_FAKE_LIBRARY").unwrap_or_default();
    let host = FakeHost(library.split(',').filter(|s| !s.is_empty()).map(String::from).collect());
    let config = Config { data_dir: data_dir.into(), port: DEFAULT_PORT, version: "dev".into() };
    let bridge = start(config, Arc::new(host)).await?;
    println!("listening on http://{}", bridge.addr());
    std::future::pending::<()>().await;
    Ok(())
}
