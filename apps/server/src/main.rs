use anyhow::{bail, Context};
use clap::{Parser, Subcommand};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use typeshelf_core::update;
use typeshelf_server::{admin, app, audit, auth, db, AppState};

#[derive(Parser)]
#[command(name = "typeshelf-server", version, about = "Glyphyard admin server")]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the server.
    Serve {
        #[arg(long, default_value_t = 8080)]
        port: u16,
        /// Address to listen on; use 127.0.0.1 behind a reverse proxy or tunnel.
        #[arg(long, default_value = "0.0.0.0")]
        host: std::net::IpAddr,
        /// Public address desktop apps use (shown in admin Settings), e.g. https://fonts.example.com.
        #[arg(long)]
        public_url: Option<String>,
        #[arg(long, default_value = "./data")]
        data: PathBuf,
        /// Built admin UI (contains index.html).
        #[arg(long, default_value = concat!(env!("CARGO_MANIFEST_DIR"), "/static"))]
        static_dir: PathBuf,
    },
    /// Create an admin, or set a new password if the email exists. Prompts for the password.
    CreateAdmin {
        email: String,
        #[arg(long, default_value = "./data")]
        data: PathBuf,
    },
    /// Print the install key to bake into desktop installers.
    InstallKey {
        #[arg(long, default_value = "./data")]
        data: PathBuf,
    },
    /// Create the key pair that signs app updates. Keep the private key file offline; bake the printed
    /// public key into the app with `scripts/build-windows.ps1 -UpdateKey`.
    UpdateKeygen {
        /// Where to write the private key (must not exist yet).
        #[arg(long)]
        out: PathBuf,
    },
    /// Write `<setup>.manifest` (version, size, BLAKE3 hash) and `<setup>.manifest.sig` for a setup program.
    UpdateSign {
        setup: PathBuf,
        /// The private key file from `update-keygen`.
        #[arg(long)]
        key: PathBuf,
        /// The setup program's version; read from a `Glyphyard-Setup-<version>.exe` (or older `Typeshelf-Setup-…`) file name when left out.
        #[arg(long)]
        version: Option<String>,
    },
}

fn open_db(data: &Path) -> anyhow::Result<rusqlite::Connection> {
    std::fs::create_dir_all(data).with_context(|| format!("creating {}", data.display()))?;
    db::open(Some(&data.join("typeshelf.db"))).context("opening the database")
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "typeshelf_server=info,tower_http=info".into()),
        )
        .init();

    match Cli::parse().command {
        Cmd::Serve { port, host, public_url, data, static_dir } => {
            let conn = open_db(&data)?;
            if let Some(url) = public_url {
                let mut s = db::settings(&conn)?;
                s.server_url = url.trim().trim_end_matches('/').to_string();
                db::save_settings(&conn, &s)?;
            }
            let state = AppState::new(conn, data.clone());
            let addr = SocketAddr::new(host, port);
            let listener = tokio::net::TcpListener::bind(addr).await.with_context(|| format!("binding {addr}"))?;
            tracing::info!("listening on http://{addr} (data in {})", data.display());
            axum::serve(listener, app(state, &static_dir)).await?;
        }
        Cmd::InstallKey { data } => println!("{}", db::settings(&open_db(&data)?)?.install_key),
        Cmd::UpdateKeygen { out } => {
            let (private, public) = update::generate_key()?;
            let mut open = std::fs::OpenOptions::new();
            open.write(true).create_new(true);
            // Readable by its owner only.
            #[cfg(unix)]
            std::os::unix::fs::OpenOptionsExt::mode(&mut open, 0o600);
            let mut f = open.open(&out).with_context(|| format!("creating {}", out.display()))?;
            std::io::Write::write_all(&mut f, format!("{private}\n").as_bytes())?;
            println!("Private key written to {}. Keep it offline; anyone who has it can ship updates.", out.display());
            println!("Public key (pass to scripts/build-windows.ps1 -UpdateKey):\n{public}");
        }
        Cmd::UpdateSign { setup, key, version } => {
            let name = setup.file_name().and_then(|n| n.to_str()).unwrap_or_default();
            let from_name = ["Glyphyard-Setup-", "Typeshelf-Setup-"].iter().find_map(|p| name.strip_prefix(p)).and_then(|v| v.strip_suffix(".exe"));
            let Some(version) = version.as_deref().or(from_name).and_then(update::Version::parse) else {
                bail!("give the version as major.minor.patch with --version (or name the file Glyphyard-Setup-<version>.exe)");
            };
            let private = std::fs::read_to_string(&key).with_context(|| format!("reading {}", key.display()))?;
            let bytes = std::fs::read(&setup).with_context(|| format!("reading {}", setup.display()))?;
            let manifest = update::Manifest::for_bytes(version, &bytes).to_text();
            let (signature, public) = update::sign(&private, manifest.as_bytes())?;
            let (m_path, s_path) = (PathBuf::from(format!("{}.manifest", setup.display())), PathBuf::from(format!("{}.manifest.sig", setup.display())));
            std::fs::write(&m_path, &manifest)?;
            std::fs::write(&s_path, format!("{signature}\n"))?;
            println!("Signed version {version} ({} bytes).\n{}\n{}\nThe app must be built with update key {public}.", bytes.len(), m_path.display(), s_path.display());
        }
        Cmd::CreateAdmin { email, data } => {
            if !email.contains('@') {
                bail!("`{email}` is not an email address");
            }
            let password = rpassword::prompt_password("Password: ")?;
            if password.chars().count() < admin::MIN_PASSWORD {
                bail!("passwords need at least {} characters", admin::MIN_PASSWORD);
            }
            if rpassword::prompt_password("Repeat password: ")? != password {
                bail!("the passwords do not match");
            }
            let conn = open_db(&data)?;
            let hash = auth::hash_password(&password);
            let first: bool = conn.query_row("SELECT NOT EXISTS(SELECT 1 FROM admins)", [], |r| r.get(0))?;
            let role = if first { "owner" } else { "admin" };
            if admin::insert_admin(&conn, &email, &hash, role)? {
                audit::record(&conn, "command line", "admin_added", email.trim(), &format!("role {role}"))?;
                println!("Created admin {email} ({role}).");
            } else {
                // The recovery path: a new password, and every session of that admin ends.
                let id: i64 = conn.query_row("SELECT id FROM admins WHERE email = ?1", [email.trim()], |r| r.get(0))?;
                admin::set_password(&conn, id, &hash)?;
                audit::record(&conn, "command line", "password_reset", email.trim(), "")?;
                println!("Updated the password for {email} and signed them out everywhere.");
            }
        }
    }
    Ok(())
}
