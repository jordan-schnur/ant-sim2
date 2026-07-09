use crate::world::World;
use std::fmt;

#[derive(Debug)]
pub enum SnapshotError {
    Encode(String),
    Decode(String),
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SnapshotError::Encode(e) => write!(f, "failed to encode snapshot: {e}"),
            SnapshotError::Decode(e) => write!(f, "failed to decode snapshot: {e}"),
        }
    }
}

impl std::error::Error for SnapshotError {}

pub fn save(world: &World) -> Result<Vec<u8>, SnapshotError> {
    bincode::serialize(world).map_err(|e| SnapshotError::Encode(e.to_string()))
}

/// The spatial index is derived, not stored, so it is rebuilt here. Forgetting
/// this yields a world that looks right and then behaves wrong on the next tick.
pub fn load(bytes: &[u8]) -> Result<World, SnapshotError> {
    let mut world: World =
        bincode::deserialize(bytes).map_err(|e| SnapshotError::Decode(e.to_string()))?;
    world.rebuild_index();
    Ok(world)
}
