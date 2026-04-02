//! Network fault injection for testing failure scenarios.

use std::collections::{BTreeMap, HashSet};
use std::time::Duration;

use crate::time::Instant;
use crate::NodeId;

/// A network fault that can be injected into the simulation.
#[derive(Clone, Debug, PartialEq)]
pub enum Fault {
    /// Network partition - only nodes within the same group can communicate.
    Partition {
        /// Groups of nodes that can communicate with each other.
        groups: Vec<Vec<NodeId>>,
    },
    /// Increase drop rate on all links.
    Drop { rate: f64 },
    /// Add latency to all links.
    Delay { min: Duration, max: Duration },
    /// Increase duplicate rate on all links.
    Duplicate { rate: f64 },
    /// Heal - remove all active faults and restore normal operation.
    Heal,
}

impl Fault {
    /// Creates a partition fault.
    pub fn partition(groups: Vec<Vec<NodeId>>) -> Self {
        Self::Partition { groups }
    }

    /// Creates a simple two-way partition (split brain).
    pub fn split(group_a: Vec<NodeId>, group_b: Vec<NodeId>) -> Self {
        Self::Partition {
            groups: vec![group_a, group_b],
        }
    }

    /// Creates a drop fault.
    pub fn drop(rate: f64) -> Self {
        Self::Drop { rate: rate.clamp(0.0, 1.0) }
    }

    /// Creates a delay fault.
    pub fn delay(min: Duration, max: Duration) -> Self {
        Self::Delay { min, max }
    }

    /// Creates a duplicate fault.
    pub fn duplicate(rate: f64) -> Self {
        Self::Duplicate { rate: rate.clamp(0.0, 1.0) }
    }

    /// Creates a heal fault (removes all faults).
    pub fn heal() -> Self {
        Self::Heal
    }
}

/// Schedule of faults to inject at specific times.
#[derive(Clone, Debug, Default)]
pub struct FaultSchedule {
    /// Faults scheduled at specific instants.
    events: BTreeMap<Instant, Vec<Fault>>,
}

impl FaultSchedule {
    /// Creates an empty fault schedule.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a fault to occur at the given instant.
    pub fn add(&mut self, at: Instant, fault: Fault) {
        self.events.entry(at).or_default().push(fault);
    }

    /// Returns all faults scheduled at exactly the given instant.
    pub fn faults_at(&self, instant: Instant) -> Vec<&Fault> {
        self.events
            .get(&instant)
            .map(|v| v.iter().collect())
            .unwrap_or_default()
    }

    /// Returns the next fault time strictly after the given instant.
    pub fn next_fault_time(&self, after: Instant) -> Option<Instant> {
        self.events
            .range((std::ops::Bound::Excluded(after), std::ops::Bound::Unbounded))
            .next()
            .map(|(k, _)| *k)
    }

    /// Returns all faults that should trigger up to and including the given instant.
    pub fn take_faults_until(&mut self, until: Instant) -> Vec<(Instant, Fault)> {
        let mut result = Vec::new();
        let to_remove: Vec<_> = self
            .events
            .range(..=until)
            .map(|(k, _)| *k)
            .collect();

        for instant in to_remove {
            if let Some(faults) = self.events.remove(&instant) {
                for fault in faults {
                    result.push((instant, fault));
                }
            }
        }
        result
    }

    /// Returns true if there are no scheduled faults.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Returns the total number of scheduled fault events.
    pub fn len(&self) -> usize {
        self.events.values().map(|v| v.len()).sum()
    }
}

/// Active fault state tracking.
#[derive(Clone, Debug, Default)]
pub struct FaultState {
    /// Current partition groups (empty = no partition).
    partition_groups: Vec<HashSet<NodeId>>,
    /// Current drop rate modifier.
    drop_rate: f64,
    /// Current delay modifiers.
    delay: Option<(Duration, Duration)>,
    /// Current duplicate rate modifier.
    duplicate_rate: f64,
}

impl FaultState {
    /// Creates a new fault state with no active faults.
    pub fn new() -> Self {
        Self::default()
    }

    /// Applies a fault to the state.
    pub fn apply(&mut self, fault: &Fault) {
        match fault {
            Fault::Partition { groups } => {
                self.partition_groups = groups
                    .iter()
                    .map(|g| g.iter().copied().collect())
                    .collect();
            }
            Fault::Drop { rate } => {
                self.drop_rate = *rate;
            }
            Fault::Delay { min, max } => {
                self.delay = Some((*min, *max));
            }
            Fault::Duplicate { rate } => {
                self.duplicate_rate = *rate;
            }
            Fault::Heal => {
                *self = Self::new();
            }
        }
    }

    /// Checks if two nodes can communicate given current partitions.
    pub fn can_communicate(&self, from: NodeId, to: NodeId) -> bool {
        if self.partition_groups.is_empty() {
            return true;
        }

        // Find if both nodes are in the same group
        for group in &self.partition_groups {
            if group.contains(&from) && group.contains(&to) {
                return true;
            }
        }
        
        false
    }

    /// Returns the current drop rate modifier.
    pub fn drop_rate(&self) -> f64 {
        self.drop_rate
    }

    /// Returns the current delay modifier, if any.
    pub fn delay(&self) -> Option<(Duration, Duration)> {
        self.delay
    }

    /// Returns the current duplicate rate modifier.
    pub fn duplicate_rate(&self) -> f64 {
        self.duplicate_rate
    }

    /// Returns true if there are any active faults.
    pub fn has_active_faults(&self) -> bool {
        !self.partition_groups.is_empty()
            || self.drop_rate > 0.0
            || self.delay.is_some()
            || self.duplicate_rate > 0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fault_constructors() {
        let _ = Fault::partition(vec![vec![1, 2], vec![3, 4]]);
        let _ = Fault::split(vec![1, 2], vec![3, 4]);
        let _ = Fault::drop(0.5);
        let _ = Fault::delay(Duration::from_millis(10), Duration::from_millis(100));
        let _ = Fault::duplicate(0.1);
        let _ = Fault::heal();
    }

    #[test]
    fn test_schedule_add_and_get() {
        let mut schedule = FaultSchedule::new();
        schedule.add(Instant::from_nanos(100), Fault::drop(0.5));
        schedule.add(Instant::from_nanos(200), Fault::heal());

        let faults = schedule.faults_at(Instant::from_nanos(100));
        assert_eq!(faults.len(), 1);
        assert!(matches!(faults[0], Fault::Drop { rate } if *rate == 0.5));
    }

    #[test]
    fn test_schedule_multiple_at_same_time() {
        let mut schedule = FaultSchedule::new();
        schedule.add(Instant::from_nanos(100), Fault::drop(0.5));
        schedule.add(Instant::from_nanos(100), Fault::duplicate(0.1));

        let faults = schedule.faults_at(Instant::from_nanos(100));
        assert_eq!(faults.len(), 2);
    }

    #[test]
    fn test_next_fault_time() {
        let mut schedule = FaultSchedule::new();
        schedule.add(Instant::from_nanos(100), Fault::drop(0.5));
        schedule.add(Instant::from_nanos(300), Fault::heal());

        assert_eq!(
            schedule.next_fault_time(Instant::from_nanos(0)),
            Some(Instant::from_nanos(100))
        );
        assert_eq!(
            schedule.next_fault_time(Instant::from_nanos(100)),
            Some(Instant::from_nanos(300))
        );
        assert_eq!(schedule.next_fault_time(Instant::from_nanos(300)), None);
    }

    #[test]
    fn test_take_faults_until() {
        let mut schedule = FaultSchedule::new();
        schedule.add(Instant::from_nanos(100), Fault::drop(0.5));
        schedule.add(Instant::from_nanos(200), Fault::duplicate(0.1));
        schedule.add(Instant::from_nanos(300), Fault::heal());

        let taken = schedule.take_faults_until(Instant::from_nanos(200));
        assert_eq!(taken.len(), 2);
        assert_eq!(schedule.len(), 1); // Only the 300ns fault remains
    }

    #[test]
    fn test_partition_blocks_communication() {
        let mut state = FaultState::new();
        
        // No partition - all can communicate
        assert!(state.can_communicate(1, 2));
        assert!(state.can_communicate(1, 3));

        // Add partition: [1, 2] and [3, 4]
        state.apply(&Fault::partition(vec![vec![1, 2], vec![3, 4]]));

        // Same group can communicate
        assert!(state.can_communicate(1, 2));
        assert!(state.can_communicate(3, 4));

        // Different groups cannot
        assert!(!state.can_communicate(1, 3));
        assert!(!state.can_communicate(2, 4));
    }

    #[test]
    fn test_heal_removes_all_faults() {
        let mut state = FaultState::new();
        state.apply(&Fault::partition(vec![vec![1], vec![2]]));
        state.apply(&Fault::drop(0.5));
        state.apply(&Fault::duplicate(0.1));
        state.apply(&Fault::delay(Duration::from_millis(10), Duration::from_millis(100)));

        assert!(state.has_active_faults());

        state.apply(&Fault::heal());

        assert!(!state.has_active_faults());
        assert!(state.can_communicate(1, 2));
        assert_eq!(state.drop_rate(), 0.0);
        assert_eq!(state.duplicate_rate(), 0.0);
        assert!(state.delay().is_none());
    }

    #[test]
    fn test_fault_state_accessors() {
        let mut state = FaultState::new();
        state.apply(&Fault::drop(0.5));
        assert_eq!(state.drop_rate(), 0.5);

        state.apply(&Fault::duplicate(0.3));
        assert_eq!(state.duplicate_rate(), 0.3);

        state.apply(&Fault::delay(Duration::from_millis(10), Duration::from_millis(20)));
        assert_eq!(state.delay(), Some((Duration::from_millis(10), Duration::from_millis(20))));
    }

    #[test]
    fn test_schedule_is_empty() {
        let mut schedule = FaultSchedule::new();
        assert!(schedule.is_empty());
        
        schedule.add(Instant::from_nanos(100), Fault::heal());
        assert!(!schedule.is_empty());
    }
}
