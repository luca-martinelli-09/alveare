use anyhow::{Context, Result};
use clap::Parser;

use alveare::commands::{run, Cli, Cmd};
use alveare::{config, Api};

#[tokio::main]
async fn main() {
    if let Err(e) = real_main().await {
        eprintln!("errore: {e:#}");
        std::process::exit(1);
    }
}

async fn real_main() -> Result<()> {
    let cli = Cli::parse();
    if let Cmd::Login { url, token } = &cli.cmd {
        let api = Api::new(url, token);
        let me = api.get("/api/me").await.context("token o URL non validi")?;
        let path = config::save(url, token)?;
        println!("accesso come {} salvato in {}", me["user"]["email"].as_str().unwrap_or("?"), path.display());
        return Ok(());
    }
    let saved = config::load();
    let url = cli.url.clone().or_else(|| saved.as_ref().map(|s| s.0.clone())).context("manca l'URL del pannello: alveare login o ALVEARE_URL")?;
    let token = cli.token.clone().or_else(|| saved.as_ref().map(|s| s.1.clone())).context("manca il token: alveare login o ALVEARE_TOKEN")?;
    let api = Api::new(&url, &token);
    run(&api, cli.cmd, cli.json, &mut std::io::stdout()).await
}
