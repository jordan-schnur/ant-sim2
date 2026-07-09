//! Runs a world with no renderer and prints per-colony stats as CSV.
//! This is where you find out whether evolution does anything.

use clap::Parser;
use sim::config::Config;
use sim::snapshot::{load, save};
use sim::world::World;
use std::io::Write;

#[derive(Parser)]
#[command(about = "Headless antsim2 runner")]
struct Args {
    #[arg(long, default_value_t = 100_000)]
    ticks: u64,
    #[arg(long, default_value_t = 1)]
    seed: u64,
    /// Emit one CSV row per colony every N ticks.
    #[arg(long, default_value_t = 1_000)]
    every: u64,
    /// Override the number of founding ants per colony.
    #[arg(long)]
    ants: Option<u32>,
    /// Resume from a snapshot instead of generating a fresh world.
    #[arg(long)]
    load: Option<String>,
    /// Write a snapshot here when the run finishes.
    #[arg(long)]
    save: Option<String>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let mut world = match &args.load {
        Some(path) => load(&std::fs::read(path)?)?,
        None => {
            let mut cfg = Config::default();
            if let Some(n) = args.ants {
                cfg.initial_ants_per_colony = n;
            }
            World::new(&cfg, args.seed)
        }
    };

    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::new(stdout.lock());
    writeln!(
        out,
        "tick,colony,population,store,births,deaths,floor_spawns,mean_size,generation,\
         food_delivered,delivered_total"
    )?;

    for _ in 0..args.ticks {
        world.tick();
        if world.tick_count % args.every == 0 {
            for s in world.stats() {
                writeln!(
                    out,
                    "{},{},{},{:.1},{},{},{},{:.3},{:.2},{:.1},{:.1}",
                    world.tick_count,
                    s.id,
                    s.population,
                    s.store,
                    s.births,
                    s.deaths,
                    s.floor_spawns,
                    s.mean_size,
                    s.mean_lineage,
                    s.food_delivered,
                    s.delivered_total
                )?;
            }
            out.flush()?;
        }
    }

    if let Some(path) = &args.save {
        std::fs::write(path, save(&world)?)?;
        eprintln!("wrote snapshot to {path}");
    }
    Ok(())
}
