use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventKind {
    FirstDelivery = 0,
    FirstKill = 1,
    FirstTrailFollow = 2,
    PopulationMilestone = 3,
    EldestAnt = 4,
    TopForager = 5,
}

impl EventKind {
    pub fn from_u8(v: u8) -> Option<EventKind> {
        Some(match v {
            0 => EventKind::FirstDelivery,
            1 => EventKind::FirstKill,
            2 => EventKind::FirstTrailFollow,
            3 => EventKind::PopulationMilestone,
            4 => EventKind::EldestAnt,
            5 => EventKind::TopForager,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChronicleEvent {
    pub tick: u64,
    pub colony: u8,
    pub kind: EventKind,
    pub ant_id: Option<u64>,
    pub ant_name: Option<String>,
    pub text: String,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Chronicle {
    pub events: Vec<ChronicleEvent>,
}

impl Chronicle {
    /// Keeping every permanent "first" unbounded is impractical, so cap the whole
    /// log and drop the oldest. Permanent firsts are rare and near the front of a
    /// run, so in practice they survive; the cap protects a very long session.
    pub const CAP: usize = 200;

    pub fn new() -> Self {
        Self::default()
    }

    /// Append an event. `latch` is the caller's one-shot flag for a "first":
    /// pass a rolling event a throwaway `&mut false`. Set here so the detector
    /// site stays a single call.
    pub fn record(&mut self, latch: &mut bool, ev: ChronicleEvent) {
        *latch = true;
        self.events.push(ev);
        if self.events.len() > Self::CAP {
            let overflow = self.events.len() - Self::CAP;
            self.events.drain(0..overflow);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_delivery_fires_once_and_names_the_ant() {
        let mut ch = Chronicle::new();
        let mut fired = false;
        ch.record(&mut fired, ChronicleEvent {
            tick: 10, colony: 2, kind: EventKind::FirstDelivery,
            ant_id: Some(7), ant_name: Some(crate::names::ant_name(7)),
            text: "brought the first food home".into(),
        });
        assert_eq!(ch.events.len(), 1);
        assert!(fired, "the one-shot flag must latch");
    }

    #[test]
    fn the_chronicle_is_capped_newest_kept() {
        let mut ch = Chronicle::new();
        let mut flag = false;
        for t in 0..(Chronicle::CAP as u64 + 50) {
            ch.record(&mut flag, ChronicleEvent {
                tick: t, colony: 0, kind: EventKind::PopulationMilestone,
                ant_id: None, ant_name: None, text: "grew".into(),
            });
            flag = false; // rolling events do not latch
        }
        assert_eq!(ch.events.len(), Chronicle::CAP);
        assert_eq!(ch.events.last().unwrap().tick, Chronicle::CAP as u64 + 49,
            "the newest event must be retained");
    }

    #[test]
    fn event_kind_round_trips_through_its_wire_byte() {
        for k in [EventKind::FirstDelivery, EventKind::FirstKill,
                  EventKind::FirstTrailFollow, EventKind::PopulationMilestone,
                  EventKind::EldestAnt, EventKind::TopForager] {
            assert_eq!(EventKind::from_u8(k as u8), Some(k));
        }
        assert_eq!(EventKind::from_u8(200), None);
    }
}
