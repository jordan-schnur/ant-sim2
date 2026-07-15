use serde::{Deserialize, Serialize};

/// Every tunable simulation rule. No magic numbers live outside this struct.
///
/// The defaults are a *starting guess*, not a tuned equilibrium. Expect to
/// sweep evaporation/diffusion and the trait taxes before anything evolves.
///
/// # The break-even calculation
///
/// The defaults below are chosen so a competent forager turns a profit. If it
/// cannot, no amount of evolution helps — the game is unwinnable and the store
/// only ever drains. Task 20's scripted-forager test guards this. The
/// arithmetic, at mean random traits (speed 0.525, strength 0.5, armor 0.5,
/// vision 4.5, carry 10.5) and size 1.0:
///
/// - upkeep/tick = 0.010 + 0.020(0.525) + 0.010(0.5) + 0.010(0.5) + 0.005(4.5)
///                = 0.053
/// - a round trip to the guaranteed patch at `SEED_PATCH_DISTANCE` = 12 cells
///   is 24 cells of travel at 0.525 cells/tick = 46 ticks, plus 10.5 food at
///   `harvest_rate` 2.0 = 5 ticks. Call it 51 ticks.
/// - trip cost = 0.053 x 51 + 0.005 x 24 = ~2.8 energy
/// - trip yield = 10.5 food, and refuelling is 1:1, so the margin is ~3.7x.
///
/// Two ratios matter, and they pull against each other:
/// - **yield / trip cost** must be comfortably > 1, or the colony starves.
/// - **`max_energy_per_size` / upkeep** is how long an unfed ant lives:
///   30 / 0.053 = ~566 ticks. Push it much past ~2000 (the minimum lifespan)
///   and starvation stops selecting for anything, because every ant dies of
///   old age with a full tank.
///
/// Worked through with the values below: upkeep 0.0530/tick (vision is still
/// 42% of it), trip 51 ticks, cost 2.82, yield 10.5 — a **3.7x margin**. An
/// unfed founder lives 566 ticks; a newborn at 60% of a size-0.5 tank lives
/// 340. Both comfortably under the 2000-tick minimum lifespan.
///
/// Earlier defaults set `tax_vision` at 0.02, which alone was over half of all
/// upkeep and made every trip net-negative (cost ~19 against a yield of 10.5).
/// If you retune, redo this sum.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Config {
    // --- World ---
    pub width: u16,
    pub height: u16,
    pub num_colonies: u8,
    pub initial_ants_per_colony: u32,
    /// Target fraction of the map covered by stone. Blob *count* is derived
    /// from this and the map area, so terrain density is scale-invariant —
    /// a 64x64 test world and the 512x512 real one look alike.
    pub stone_density: f32,
    pub stone_blob_radius: f32,

    // --- Colony economy ---
    pub initial_food_store: f32,
    /// Food store spent to spawn one ant.
    pub birth_cost: f32,
    pub max_births_per_tick: u32,
    /// Below this population, the nest spawns free ants from the hall of fame.
    pub extinction_floor: u32,
    /// Minimum ticks between two free floor spawns for the same colony. Without
    /// this the floor tops a colony back up *in the tick its ants die*, which
    /// hands a besieging colony an infinite conveyor of free corpses to
    /// scavenge — energy created from nothing at a fixed, findable location.
    pub floor_respawn_interval: u64,
    pub hall_of_fame_size: usize,
    /// Energy per tick an ant regains while standing on its own nest.
    pub refuel_rate: f32,

    // --- Pheromones (per-tick decay multipliers, in (0,1)) ---
    pub food_evaporation: f32,
    pub alarm_evaporation: f32,
    pub scent_evaporation: f32,
    /// Fraction of the neighbour-average blended in per tick, per layer.
    pub food_diffusion: f32,
    pub alarm_diffusion: f32,
    pub scent_diffusion: f32,
    /// The exploration/home trail decays and spreads like the food trail. Kept
    /// off the tunable rail for now (a fixed constant); promote to
    /// `CONFIG_FIELDS` if it wants live tuning.
    pub home_evaporation: f32,
    pub home_diffusion: f32,
    /// Home-trail deposited per tick by an *unladen* ant, the complement to
    /// `food_trail_emission` (laid by laden ants). Unladen ants cluster at and
    /// radiate from the nest, so this field peaks homeward.
    pub home_trail_emission: f32,
    /// Food-trail deposited per unit of carried food, per tick.
    pub food_trail_emission: f32,
    /// Alarm deposited when an ant attacks or is damaged.
    pub alarm_emission: f32,
    /// Colony scent deposited by every ant, every tick.
    pub ant_scent_emission: f32,
    /// Colony scent deposited by each nest tile, every tick. Much larger than
    /// `ant_scent_emission`: this is the beacon ants climb to get home.
    pub nest_scent_emission: f32,
    /// Divisor for the logarithmic pheromone sensor compression (see
    /// `sense::squash_phero`). Pheromone magnitudes span four orders of
    /// magnitude between a stale trail and a nest tile; a linear or tanh
    /// squash saturates near the nest and erases the very gradient an ant
    /// needs to find its way home.
    pub phero_log_div: f32,

    // --- Metabolism and the trait tax ---
    pub base_upkeep: f32,
    pub tax_speed: f32,
    pub tax_strength: f32,
    pub tax_armor: f32,
    pub tax_vision: f32,
    /// Energy per cell of distance moved.
    pub move_cost: f32,

    // --- Combat ---
    pub attack_cost: f32,
    pub attack_damage: f32,
    /// Fraction of a victim's remaining energy the killer absorbs.
    pub kill_energy_frac: f32,

    // --- Growth ---
    pub max_energy_per_size: f32,
    /// Fraction of max energy above which an ant converts energy into size.
    pub growth_threshold: f32,
    pub growth_rate: f32,
    pub shrink_rate: f32,

    // --- Fitness shaping ---
    /// Weight on lifetime food *harvested* in the selection fitness, relative to
    /// food *delivered* (weight 1.0). A dense gradient toward delivery: an ant
    /// that finds and grabs food is closer to a forager than one that never
    /// moves. Kept small so any real delivery dominates a lifetime of mere
    /// harvesting. `0.0` recovers the original delivery-only thesis exactly.
    pub harvest_weight: f32,
    /// Weight on lifetime *homing* progress — distance an ant carried food back
    /// toward its own nest — in the selection fitness, relative to delivered
    /// (weight 1.0). This is the gradient the delivery-only signal lacks: an ant
    /// that grabs food and hauls it homeward, even without completing a drop, is
    /// closer to a forager than one that never heads home. It is what lets a
    /// colony bootstrap foraging from random genomes. `0.0` disables it.
    pub homing_weight: f32,

    // --- Mutation ---
    /// Fraction of parameters perturbed per birth.
    pub mutation_rate: f32,
    pub mutation_sigma: f32,
    pub big_jump_chance: f32,
    pub big_jump_sigma: f32,

    // --- Food ---
    pub food_patch_count: u32,
    pub food_patch_radius: f32,
    pub food_patch_max: f32,
    pub food_regrow: f32,
    /// Food harvested per tick by an ant standing on a food cell.
    pub harvest_rate: f32,
}

impl Config {
    pub fn cell_count(&self) -> usize {
        self.width as usize * self.height as usize
    }

    /// Selection fitness: the real objective (food delivered) plus the harvest
    /// and homing nudges that give evolution a gradient before any full delivery
    /// happens. `harvest_weight = homing_weight = 0` recovers pure delivery.
    #[inline]
    pub fn fitness(&self, delivered: f32, harvested: f32, homing: f32) -> f32 {
        delivered + self.harvest_weight * harvested + self.homing_weight * homing
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            width: 512,
            height: 512,
            num_colonies: 8,
            initial_ants_per_colony: 40,
            stone_density: 0.06,
            stone_blob_radius: 7.0,

            // Enough to refuel through a bad stretch, but NOT a birth windfall:
            // at birth_cost 12 this buys ~12 births, not 100. A huge initial
            // store just converts to a population spike that then starves.
            initial_food_store: 150.0,
            // birth_cost and refuel_rate were tuned by seed-averaged headless
            // sweeps to make colonies actually grow past the extinction floor.
            // The old defaults (40 / 2.0) pinned every colony at 5 ants: no
            // realistic delivery rate could bank a 40-food birth, and refuel at
            // 2.0/tick let loitering ants drain the store as fast as it filled.
            // At 12 / 0.75 a modest surplus buys a birth and delivered food
            // accumulates, so a couple of colonies per run climb to ~40 ants
            // with sustained paid births. See the 2026-07-13 economy-tuning note.
            birth_cost: 12.0,
            max_births_per_tick: 2,
            extinction_floor: 5,
            floor_respawn_interval: 200,
            hall_of_fame_size: 10,
            refuel_rate: 0.75,

            food_evaporation: 0.995,
            alarm_evaporation: 0.97,
            scent_evaporation: 0.999,
            food_diffusion: 0.12,
            alarm_diffusion: 0.20,
            scent_diffusion: 0.06,
            // The home trail should linger a little longer and spread a little
            // wider than the food trail, so an outbound network of routes builds
            // up around the nest rather than a set of thin one-ant tracks.
            home_evaporation: 0.997,
            home_diffusion: 0.15,
            home_trail_emission: 2.0,
            food_trail_emission: 2.0,
            alarm_emission: 5.0,
            ant_scent_emission: 0.5,
            nest_scent_emission: 50.0,
            phero_log_div: 12.0,

            // See the break-even note above before touching these. `tax_vision`
            // is multiplied by a trait ranging to 8.0, so it is worth ~8x its
            // face value relative to the 0..1 traits.
            base_upkeep: 0.010,
            tax_speed: 0.020,
            tax_strength: 0.010,
            tax_armor: 0.010,
            tax_vision: 0.005,
            move_cost: 0.005,

            attack_cost: 0.5,
            attack_damage: 4.0,
            kill_energy_frac: 0.3,

            max_energy_per_size: 30.0,
            growth_threshold: 0.8,
            growth_rate: 0.002,
            shrink_rate: 0.004,

            harvest_weight: 0.02,
            homing_weight: 0.05,

            mutation_rate: 0.08,
            mutation_sigma: 0.05,
            big_jump_chance: 0.002,
            big_jump_sigma: 0.5,

            food_patch_count: 40,
            food_patch_radius: 6.0,
            food_patch_max: 200.0,
            food_regrow: 0.002,
            harvest_rate: 2.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_grid_is_512_squared() {
        let c = Config::default();
        assert_eq!(c.width, 512);
        assert_eq!(c.height, 512);
        assert_eq!(c.cell_count(), 262_144);
    }

    #[test]
    fn default_has_eight_colonies() {
        assert_eq!(Config::default().num_colonies, 8);
    }

    #[test]
    fn evaporation_rates_are_decay_multipliers() {
        let c = Config::default();
        for r in [c.food_evaporation, c.alarm_evaporation, c.scent_evaporation, c.home_evaporation] {
            assert!(r > 0.0 && r < 1.0, "evaporation must be in (0,1), got {r}");
        }
    }

    #[test]
    fn roundtrips_through_serde() {
        let c = Config::default();
        let bytes = bincode::serialize(&c).unwrap();
        assert_eq!(c, bincode::deserialize::<Config>(&bytes).unwrap());
    }

    /// Mean upkeep per tick at mean random traits, size 1.0. Mirrors
    /// `Genome::upkeep` without depending on it, so a change to either side
    /// of the economy trips this test rather than passing silently.
    fn mean_upkeep(c: &Config) -> f32 {
        c.base_upkeep
            + c.tax_speed * 0.525
            + c.tax_strength * 0.5
            + c.tax_armor * 0.5
            + c.tax_vision * 4.5
    }

    #[test]
    fn a_mean_forager_turns_a_profit_on_a_round_trip() {
        // The single most important invariant in the whole config: if a trip
        // costs more than it yields, no amount of evolution can save the
        // colony, and every downstream test is testing a corpse.
        let c = Config::default();
        let travel = 2.0 * 12.0; // to SEED_PATCH_DISTANCE and back
        let ticks = travel / 0.525 + 10.5 / c.harvest_rate;
        let cost = mean_upkeep(&c) * ticks + c.move_cost * travel;
        let yield_ = 10.5; // mean carry_capacity
        assert!(yield_ > 2.0 * cost, "trip yields {yield_} but costs {cost}");
    }

    #[test]
    fn starvation_bites_well_before_old_age() {
        // If an unfed ant outlives its minimum lifespan, starvation stops
        // selecting for anything.
        let c = Config::default();
        let ticks_to_starve = c.max_energy_per_size / mean_upkeep(&c);
        assert!(
            ticks_to_starve < 2000.0,
            "unfed ant survives {ticks_to_starve} ticks"
        );
        assert!(
            ticks_to_starve > 200.0,
            "ants starve too fast to ever reach food"
        );
    }

    #[test]
    fn the_initial_store_is_a_fuel_reserve_not_a_birth_windfall() {
        let c = Config::default();
        let instant_births = c.initial_food_store / c.birth_cost;
        assert!(
            instant_births < 25.0,
            "{instant_births} free births at t=0 is a population spike"
        );
    }

    #[test]
    fn harvest_weight_defaults_to_a_small_nudge() {
        assert_eq!(Config::default().harvest_weight, 0.02);
    }

    #[test]
    fn fitness_is_delivery_plus_weighted_harvest_and_homing() {
        let c = Config { harvest_weight: 0.02, homing_weight: 0.05, ..Config::default() };
        // 10 + 0.02*100 + 0.05*20 = 13.0
        assert!((c.fitness(10.0, 100.0, 20.0) - 13.0).abs() < 1e-6);
    }

    #[test]
    fn fitness_with_zero_weights_is_pure_delivery() {
        // The purity toggle: both nudges at 0 recovers the original thesis.
        let c = Config { harvest_weight: 0.0, homing_weight: 0.0, ..Config::default() };
        assert_eq!(c.fitness(7.0, 999.0, 999.0), 7.0);
    }

    #[test]
    fn a_single_delivery_outweighs_a_lifetime_of_harvesting() {
        // Anti-reward-hacking bound: any delivered unit must beat a plausible
        // lifetime of harvest-without-delivery at the default weight.
        let c = Config::default();
        let lifetime_harvest_only = c.fitness(0.0, 400.0, 0.0); // busy forager, never delivers
        let one_delivery = c.fitness(10.0, 0.0, 0.0);
        assert!(one_delivery > lifetime_harvest_only,
            "delivery {one_delivery} must dominate harvest {lifetime_harvest_only}");
    }
}
