#![forbid(unsafe_code)]

/// Number of sensory inputs fed to every ant's network.
pub const N_INPUTS: usize = 44;
/// First hidden layer width.
pub const N_HIDDEN1: usize = 16;
/// Second hidden layer width.
pub const N_HIDDEN2: usize = 16;
/// Network outputs: turn, throttle, attack, grab, + 4 recurrent memory values.
pub const N_OUTPUTS: usize = 8;
/// Recurrent memory values carried between ticks.
pub const N_MEMORY: usize = 4;

/// Total f32 parameters in one brain: weights + biases.
pub const N_PARAMS: usize = N_INPUTS * N_HIDDEN1
    + N_HIDDEN1
    + N_HIDDEN1 * N_HIDDEN2
    + N_HIDDEN2
    + N_HIDDEN2 * N_OUTPUTS
    + N_OUTPUTS;

pub mod ants;
pub mod apply;
pub mod brain;
pub mod colony;
pub mod config;
pub mod genome;
pub mod grid;
pub mod intent;
pub mod pheromone;
pub mod reproduce;
pub mod rng;
pub mod sense;
pub mod spatial;
pub mod stats;
pub mod world;
pub mod worldgen;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn param_count_matches_spec() {
        assert_eq!(N_PARAMS, 1128);
    }
}
