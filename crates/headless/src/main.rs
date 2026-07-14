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
    /// Override a config field, e.g. `--set birth_cost=20 --set refuel_rate=1`.
    /// Repeatable. The point of this runner is economy tuning; this is how you
    /// sweep the levers without recompiling.
    #[arg(long = "set", value_name = "FIELD=VALUE")]
    overrides: Vec<String>,
    /// Resume from a snapshot instead of generating a fresh world.
    #[arg(long)]
    load: Option<String>,
    /// Write a snapshot here when the run finishes.
    #[arg(long)]
    save: Option<String>,
}

/// Apply one `field=value` override to a config. Only the economy-relevant
/// levers are wired up — the ones a sweep actually touches. An unknown field
/// or unparseable value is an error rather than a silent no-op, so a typo in a
/// sweep script fails loudly instead of quietly measuring the defaults.
fn apply_override(cfg: &mut Config, spec: &str) -> Result<(), String> {
    let (field, value) = spec
        .split_once('=')
        .ok_or_else(|| format!("override '{spec}' is not FIELD=VALUE"))?;
    let f = |v: &str| v.parse::<f32>().map_err(|e| format!("{field}: {e}"));
    let u = |v: &str| v.parse::<u32>().map_err(|e| format!("{field}: {e}"));
    match field {
        "initial_food_store" => cfg.initial_food_store = f(value)?,
        "birth_cost" => cfg.birth_cost = f(value)?,
        "max_births_per_tick" => cfg.max_births_per_tick = u(value)?,
        "extinction_floor" => cfg.extinction_floor = u(value)?,
        "refuel_rate" => cfg.refuel_rate = f(value)?,
        "base_upkeep" => cfg.base_upkeep = f(value)?,
        "tax_speed" => cfg.tax_speed = f(value)?,
        "tax_strength" => cfg.tax_strength = f(value)?,
        "tax_armor" => cfg.tax_armor = f(value)?,
        "tax_vision" => cfg.tax_vision = f(value)?,
        "move_cost" => cfg.move_cost = f(value)?,
        "max_energy_per_size" => cfg.max_energy_per_size = f(value)?,
        "growth_threshold" => cfg.growth_threshold = f(value)?,
        "harvest_weight" => cfg.harvest_weight = f(value)?,
        "homing_weight" => cfg.homing_weight = f(value)?,
        "food_patch_count" => cfg.food_patch_count = u(value)?,
        "food_patch_max" => cfg.food_patch_max = f(value)?,
        "food_regrow" => cfg.food_regrow = f(value)?,
        "harvest_rate" => cfg.harvest_rate = f(value)?,
        "initial_ants_per_colony" => cfg.initial_ants_per_colony = u(value)?,
        _ => return Err(format!("unknown config field '{field}'")),
    }
    Ok(())
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
            for spec in &args.overrides {
                apply_override(&mut cfg, spec)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_overrides_a_known_field() {
        let mut cfg = Config::default();
        apply_override(&mut cfg, "birth_cost=17.5").unwrap();
        assert_eq!(cfg.birth_cost, 17.5);
    }

    #[test]
    fn an_unknown_field_is_an_error_not_a_silent_noop() {
        let mut cfg = Config::default();
        assert!(apply_override(&mut cfg, "nonsense=1").is_err());
    }

    #[test]
    fn a_missing_equals_is_an_error() {
        let mut cfg = Config::default();
        assert!(apply_override(&mut cfg, "birth_cost").is_err());
    }

    #[test]
    fn an_unparseable_value_is_an_error() {
        let mut cfg = Config::default();
        assert!(apply_override(&mut cfg, "birth_cost=abc").is_err());
    }

    #[test]
    fn a_uint_field_parses() {
        let mut cfg = Config::default();
        apply_override(&mut cfg, "extinction_floor=3").unwrap();
        assert_eq!(cfg.extinction_floor, 3);
    }
}
