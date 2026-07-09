use crate::genome::Genome;
use crate::{N_HIDDEN1, N_HIDDEN2, N_INPUTS, N_OUTPUTS};

pub const OUT_TURN: usize = 0;
pub const OUT_THROTTLE: usize = 1;
pub const OUT_ATTACK: usize = 2;
pub const OUT_GRAB: usize = 3;
/// Outputs `[OUT_MEMORY .. N_OUTPUTS)` are recurrent state, fed back as the
/// final `N_MEMORY` inputs on the next tick.
pub const OUT_MEMORY: usize = 4;

/// Every layer's activation. The inspector in Plan 2 renders all of these, so
/// the forward pass returns them rather than only the outputs.
#[derive(Clone, Debug)]
pub struct Activations {
    pub inputs: [f32; N_INPUTS],
    pub h1: [f32; N_HIDDEN1],
    pub h2: [f32; N_HIDDEN2],
    pub outputs: [f32; N_OUTPUTS],
}

pub trait Brain {
    fn forward(&self, inputs: &[f32; N_INPUTS]) -> Activations;
}

/// Parameter layout, in order:
///   W1 [N_INPUTS  x N_HIDDEN1], B1 [N_HIDDEN1],
///   W2 [N_HIDDEN1 x N_HIDDEN2], B2 [N_HIDDEN2],
///   W3 [N_HIDDEN2 x N_OUTPUTS], B3 [N_OUTPUTS]
impl Brain for Genome {
    fn forward(&self, inputs: &[f32; N_INPUTS]) -> Activations {
        let p = &self.params;
        let (w1, rest) = p.split_at(N_INPUTS * N_HIDDEN1);
        let (b1, rest) = rest.split_at(N_HIDDEN1);
        let (w2, rest) = rest.split_at(N_HIDDEN1 * N_HIDDEN2);
        let (b2, rest) = rest.split_at(N_HIDDEN2);
        let (w3, b3) = rest.split_at(N_HIDDEN2 * N_OUTPUTS);

        let mut h1 = [0.0f32; N_HIDDEN1];
        for (j, hj) in h1.iter_mut().enumerate() {
            let mut acc = b1[j];
            for (i, x) in inputs.iter().enumerate() {
                acc += x * w1[i * N_HIDDEN1 + j];
            }
            *hj = acc.tanh();
        }

        let mut h2 = [0.0f32; N_HIDDEN2];
        for (j, hj) in h2.iter_mut().enumerate() {
            let mut acc = b2[j];
            for (i, x) in h1.iter().enumerate() {
                acc += x * w2[i * N_HIDDEN2 + j];
            }
            *hj = acc.tanh();
        }

        let mut outputs = [0.0f32; N_OUTPUTS];
        for (j, oj) in outputs.iter_mut().enumerate() {
            let mut acc = b3[j];
            for (i, x) in h2.iter().enumerate() {
                acc += x * w3[i * N_OUTPUTS + j];
            }
            *oj = acc.tanh();
        }

        Activations {
            inputs: *inputs,
            h1,
            h2,
            outputs,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::genome::Genome;
    use crate::rng::Pcg32;
    use crate::{N_INPUTS, N_MEMORY, N_OUTPUTS};

    #[test]
    fn output_indices_leave_room_for_memory() {
        assert_eq!(OUT_MEMORY + N_MEMORY, N_OUTPUTS);
    }

    #[test]
    fn forward_is_pure() {
        let g = Genome::random(&mut Pcg32::new(1, 1));
        let inputs = [0.3f32; N_INPUTS];
        let a = g.forward(&inputs);
        let b = g.forward(&inputs);
        assert_eq!(a.outputs, b.outputs);
    }

    #[test]
    fn all_outputs_are_bounded_by_tanh() {
        let g = Genome::random(&mut Pcg32::new(2, 2));
        let inputs = [1e6f32; N_INPUTS];
        for o in g.forward(&inputs).outputs {
            assert!((-1.0..=1.0).contains(&o), "output {o} escaped tanh range");
            assert!(o.is_finite());
        }
    }

    #[test]
    fn different_inputs_give_different_outputs() {
        let g = Genome::random(&mut Pcg32::new(3, 3));
        let a = g.forward(&[0.0; N_INPUTS]);
        let b = g.forward(&[1.0; N_INPUTS]);
        assert!(a.outputs != b.outputs);
    }

    #[test]
    fn a_zero_genome_outputs_zero() {
        let mut g = Genome::random(&mut Pcg32::new(4, 4));
        g.params.iter_mut().for_each(|p| *p = 0.0);
        for o in g.forward(&[0.7; N_INPUTS]).outputs {
            assert_eq!(o, 0.0);
        }
    }

    #[test]
    fn activations_expose_every_layer() {
        let g = Genome::random(&mut Pcg32::new(5, 5));
        let a = g.forward(&[0.2; N_INPUTS]);
        assert_eq!(a.inputs.len(), N_INPUTS);
        assert_eq!(a.h1.len(), crate::N_HIDDEN1);
        assert_eq!(a.h2.len(), crate::N_HIDDEN2);
        assert_eq!(a.outputs.len(), N_OUTPUTS);
    }
}
