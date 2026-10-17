use clap::{Parser, Subcommand};
use spielstand_server::{AppState, Settings, auth, importer, mirror};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(
    name = "spielstand-server",
    version,
    about = "Keeps game saves in sync between PCs"
)]
struct Cli {
    #[command(flatten)]
    settings: Settings,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Run the server (the default).
    Serve,
    /// Rebuild an empty server from its mirror (SPIELSTAND_MIRROR).
    RestoreFromMirror,
    /// Take over a Ludusavi backup folder, or `remote:path` through rclone.
    ImportLudusavi { source: String },
    /// Set a new admin password and end all sessions.
    ResetPassword { password: String },
    /// Exit 0 when the server on SPIELSTAND_LISTEN answers (for the container health check).
    Health,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();
    let cli = Cli::parse();
    match cli.command.unwrap_or(Command::Serve) {
        Command::Serve => spielstand_server::serve(cli.settings).await,
        Command::RestoreFromMirror => {
            let spec = cli
                .settings
                .mirror
                .clone()
                .ok_or_else(|| anyhow::anyhow!("SPIELSTAND_MIRROR is not set"))?;
            let rclone = cli.settings.rclone.clone();
            let state = AppState::open(cli.settings).await?;
            tokio::fs::create_dir_all(state.settings.tmp_dir()).await?;
            let n = mirror::restore(&state, mirror::remote(&spec, &rclone).as_ref()).await?;
            println!("restored {n} versions; pair the devices again");
            Ok(())
        }
        Command::ImportLudusavi { source } => {
            let state = AppState::open(cli.settings).await?;
            let report = importer::import_source(&state, &source)
                .await
                .map_err(|e| anyhow::anyhow!("{e:?}"))?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(())
        }
        Command::Health => health(&cli.settings.listen),
        Command::ResetPassword { password } => {
            anyhow::ensure!(password.chars().count() >= 8, "at least 8 characters");
            let state = AppState::open(cli.settings).await?;
            sqlx::query("INSERT INTO admin (id, password_hash) VALUES (1, ?) ON CONFLICT (id) DO UPDATE SET password_hash = excluded.password_hash")
                .bind(auth::hash_password(&password)?)
                .execute(&state.db)
                .await?;
            sqlx::query("DELETE FROM sessions").execute(&state.db).await?;
            println!("password changed");
            Ok(())
        }
    }
}

/// A plain HTTP request without a client library: the image has no curl.
fn health(listen: &str) -> anyhow::Result<()> {
    use std::io::{Read, Write};
    let port = listen.rsplit(':').next().unwrap_or("8467");
    let mut stream = std::net::TcpStream::connect(format!("127.0.0.1:{port}"))?;
    stream.set_read_timeout(Some(std::time::Duration::from_secs(4)))?;
    stream.write_all(b"GET /healthz HTTP/1.0\r\nHost: localhost\r\n\r\n")?;
    let mut answer = String::new();
    stream.read_to_string(&mut answer)?;
    anyhow::ensure!(
        answer.starts_with("HTTP/1.1 200") || answer.starts_with("HTTP/1.0 200"),
        "unhealthy"
    );
    Ok(())
}
