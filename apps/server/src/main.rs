use anyhow::{bail, Context};
use clap::{Parser, Subcommand};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use typeshelf_server::{admin, app, auth, db, AppState};

#[derive(Parser)]
#[command(name = "typeshelf-server", version, about = "Typeshelf admin server")]
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
        Cmd::Serve { port, host, data, static_dir } => {
            let conn = open_db(&data)?;
            let state = AppState::new(conn, data.clone());
            let addr = SocketAddr::new(host, port);
            let listener = tokio::net::TcpListener::bind(addr).await.with_context(|| format!("binding {addr}"))?;
            tracing::info!("listening on http://{addr} (data in {})", data.display());
            axum::serve(listener, app(state, &static_dir)).await?;
        }
        Cmd::InstallKey { data } => println!("{}", db::settings(&open_db(&data)?)?.install_key),
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
            if admin::insert_admin(&conn, &email, &hash, if first { "owner" } else { "admin" })? {
                println!("Created admin {email}.");
            } else {
                conn.execute("UPDATE admins SET password_hash = ?1 WHERE email = ?2", [&hash, email.trim()])?;
                println!("Updated the password for {email}.");
            }
        }
    }
    Ok(())
}
