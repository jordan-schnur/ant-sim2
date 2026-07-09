#![forbid(unsafe_code)]

use clap::Parser;
use server::sim_thread;
use server::ws;
use sim::config::Config;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(about = "Serve the ant simulation over a WebSocket")]
struct Args {
    #[arg(long, default_value_t = 8080)]
    port: u16,

    #[arg(long, default_value_t = 1)]
    seed: u64,

    /// Founders per colony. Handy for a fast smoke test.
    #[arg(long)]
    ants: Option<u32>,

    /// Snapshot path used by the save and load buttons.
    #[arg(long, default_value = "snapshot.bin")]
    save: PathBuf,

    /// Load this snapshot at startup instead of generating a world.
    #[arg(long)]
    load: Option<PathBuf>,

    /// Built web client to serve. Unset means socket only (Vite serves the app
    /// in development and proxies /ws here).
    #[arg(long)]
    web: Option<PathBuf>,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let args = Args::parse();

    let mut cfg = Config::default();
    if let Some(n) = args.ants {
        cfg.initial_ants_per_colony = n;
    }

    // `--load` reuses the running server's load path rather than a separate
    // code path: start from the snapshot's own config so a snapshot taken with
    // tuned sliders comes back tuned.
    let (cfg, seed) = match &args.load {
        Some(p) => match std::fs::read(p).map(|b| sim::snapshot::load(&b)) {
            Ok(Ok(w)) => {
                tracing::info!(?p, tick = w.tick_count, "loaded snapshot");
                (w.cfg, args.seed)
            }
            Ok(Err(e)) => {
                tracing::error!(%e, ?p, "snapshot decode failed; starting fresh");
                (cfg, args.seed)
            }
            Err(e) => {
                tracing::error!(%e, ?p, "snapshot read failed; starting fresh");
                (cfg, args.seed)
            }
        },
        None => (cfg, args.seed),
    };

    let handles = sim_thread::spawn(cfg, seed, args.save.clone());

    // A `--load` on the command line means "come up showing that world", so
    // replay the load through the sim thread now that it owns the World.
    if args.load.is_some() {
        let _ = handles.commands.send(server::protocol::Command::Load);
    }

    let app = ws::router(handles, args.web);
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], args.port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .unwrap_or_else(|e| panic!("cannot bind {addr}: {e}"));

    tracing::info!("listening on http://{addr}  (ws://{addr}/ws)");
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
            tracing::info!("shutting down");
        })
        .await
        .unwrap();
}
