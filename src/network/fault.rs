//! Network fault injection for testing failure scenarios.

use std::collections::{BTreeMap, HashMap, HashSet};
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
    /// Clock skew - makes a node's clock run faster or slower.
    /// rate > 1.0 means clock runs fast, < 1.0 means slow.
    ClockSkew { 
        node: NodeId, 
        /// Rate multiplier (1.0 = normal, 2.0 = 2x fast, 0.5 = 2x slow)
        rate: f64 
    },
    /// Clock jump - instantly moves a node's clock forward or backward.
    ClockJump { 
        node: NodeId, 
        /// Amount to jump (positive = forward, negative = backward)
        delta: i64 
    },
    /// Crash a node.
    Crash { node: NodeId },
    /// Restart a crashed node.
    Restart { node: NodeId },
    /// Inject disk read errors.
    DiskReadError { rate: f64 },
    /// Inject disk write errors.
    DiskWriteError { rate: f64 },
    /// Simulate full disk (all writes fail).
    FullDisk,
    /// Corrupt data with given probability.
    Corrupt { rate: f64 },
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

    /// Creates a clock skew fault.
    /// rate > 1.0 means clock runs fast, < 1.0 means slow.
    pub fn clock_skew(node: NodeId, rate: f64) -> Self {
        Self::ClockSkew { node, rate: rate.max(0.01) }
    }

    /// Creates a clock jump fault.
    /// delta_nanos > 0 jumps forward, < 0 jumps backward.
    pub fn clock_jump(node: NodeId, delta_nanos: i64) -> Self {
        Self::ClockJump { node, delta: delta_nanos }
    }

    /// Creates a clock jump fault from a duration (forward only).
    pub fn clock_jump_forward(node: NodeId, amount: Duration) -> Self {
        Self::ClockJump { node, delta: amount.as_nanos() as i64 }
    }

    /// Creates a crash fault.
    pub fn crash(node: NodeId) -> Self {
        Self::Crash { node }
    }

    /// Creates a restart fault.
    pub fn restart(node: NodeId) -> Self {
        Self::Restart { node }
    }

    /// Creates a disk read error fault.
    pub fn disk_read_error(rate: f64) -> Self {
        Self::DiskReadError { rate: rate.clamp(0.0, 1.0) }
    }

    /// Creates a disk write error fault.
    pub fn disk_write_error(rate: f64) -> Self {
        Self::DiskWriteError { rate: rate.clamp(0.0, 1.0) }
    }

    /// Creates a full disk fault.
    pub fn full_disk() -> Self {
        Self::FullDisk
    }

    /// Creates a data corruption fault.
    pub fn corrupt(rate: f64) -> Self {
        Self::Corrupt { rate: rate.clamp(0.0, 1.0) }
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
    /// Clock skew rates per node (1.0 = normal).
    clock_skews: HashMap<NodeId, f64>,
    /// Accumulated clock offsets per node from jumps (in nanos).
    clock_offsets: HashMap<NodeId, i64>,
    /// Crashed nodes.
    crashed_nodes: HashSet<NodeId>,
    /// Disk read error rate.
    disk_read_error_rate: f64,
    /// Disk write error rate.
    disk_write_error_rate: f64,
    /// Full disk simulation active.
    full_disk: bool,
    /// Data corruption rate.
    corruption_rate: f64,
}

impl FaultState {
    /// Creates a new fault state with no active faults.
    pub fn new() -> Self {
        Self::default()
    }

    /// Applies a fault to the state.
    /// Returns Some(Fault) if the fault requires external handling (e.g., crash/restart).
    pub fn apply(&mut self, fault: &Fault) -> Option<Fault> {
        match fault {
            Fault::Partition { groups } => {
                self.partition_groups = groups
                    .iter()
                    .map(|g| g.iter().copied().collect())
                    .collect();
                None
            }
            Fault::Drop { rate } => {
                self.drop_rate = *rate;
                None
            }
            Fault::Delay { min, max } => {
                self.delay = Some((*min, *max));
                None
            }
            Fault::Duplicate { rate } => {
                self.duplicate_rate = *rate;
                None
            }
            Fault::ClockSkew { node, rate } => {
                self.clock_skews.insert(*node, *rate);
                None
            }
            Fault::ClockJump { node, delta } => {
                let current = self.clock_offsets.get(node).copied().unwrap_or(0);
                self.clock_offsets.insert(*node, current + delta);
                None
            }
            Fault::Crash { node } => {
                self.crashed_nodes.insert(*node);
                Some(fault.clone())
            }
            Fault::Restart { node } => {
                self.crashed_nodes.remove(node);
                Some(fault.clone())
            }
            Fault::DiskReadError { rate } => {
                self.disk_read_error_rate = *rate;
                None
            }
            Fault::DiskWriteError { rate } => {
                self.disk_write_error_rate = *rate;
                None
            }
            Fault::FullDisk => {
                self.full_disk = true;
                None
            }
            Fault::Corrupt { rate } => {
                self.corruption_rate = *rate;
                None
            }
            Fault::Heal => {
                *self = Self::new();
                None
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
            || !self.clock_skews.is_empty()
            || !self.clock_offsets.is_empty()
            || !self.crashed_nodes.is_empty()
            || self.disk_read_error_rate > 0.0
            || self.disk_write_error_rate > 0.0
            || self.full_disk
            || self.corruption_rate > 0.0
    }

    /// Returns the clock skew rate for a node (1.0 if no skew).
    pub fn clock_skew(&self, node: NodeId) -> f64 {
        self.clock_skews.get(&node).copied().unwrap_or(1.0)
    }

    /// Returns the accumulated clock offset for a node (0 if no offset).
    pub fn clock_offset(&self, node: NodeId) -> i64 {
        self.clock_offsets.get(&node).copied().unwrap_or(0)
    }

    /// Calculates the adjusted time for a node given the base simulated time.
    /// Takes into account both clock skew and clock jumps.
    pub fn adjusted_time(&self, node: NodeId, base_nanos: u64, elapsed_nanos: u64) -> u64 {
        let skew = self.clock_skew(node);
        let offset = self.clock_offset(node);
        
        // Apply skew to elapsed time
        let skewed_elapsed = (elapsed_nanos as f64 * skew) as u64;
        
        // Apply offset
        
        
        if offset >= 0 {
            base_nanos.saturating_add(skewed_elapsed).saturating_add(offset as u64)
        } else {
            base_nanos.saturating_add(skewed_elapsed).saturating_sub((-offset) as u64)
        }
    }

    /// Clears clock skew for a specific node.
    pub fn clear_clock_skew(&mut self, node: NodeId) {
        self.clock_skews.remove(&node);
    }

    /// Clears clock offset for a specific node.
    pub fn clear_clock_offset(&mut self, node: NodeId) {
        self.clock_offsets.remove(&node);
    }

    /// Check if a node is crashed.
    pub fn is_crashed(&self, node: NodeId) -> bool {
        self.crashed_nodes.contains(&node)
    }

    /// Get all crashed nodes.
    pub fn crashed_nodes(&self) -> &HashSet<NodeId> {
        &self.crashed_nodes
    }

    /// Get disk read error rate.
    pub fn disk_read_error_rate(&self) -> f64 {
        self.disk_read_error_rate
    }

    /// Get disk write error rate.
    pub fn disk_write_error_rate(&self) -> f64 {
        self.disk_write_error_rate
    }

    /// Check if full disk simulation is active.
    pub fn is_full_disk(&self) -> bool {
        self.full_disk
    }

    /// Get corruption rate.
    pub fn corruption_rate(&self) -> f64 {
        self.corruption_rate
    }

    /// Check if a disk read should fail based on current error rate.
    pub fn should_fail_read(&self, rng: &mut impl rand::Rng) -> bool {
        self.disk_read_error_rate > 0.0 && rng.gen::<f64>() < self.disk_read_error_rate
    }

    /// Check if a disk write should fail based on current error rate or full disk.
    pub fn should_fail_write(&self, rng: &mut impl rand::Rng) -> bool {
        self.full_disk || (self.disk_write_error_rate > 0.0 && rng.gen::<f64>() < self.disk_write_error_rate)
    }

    /// Check if data should be corrupted based on corruption rate.
    pub fn should_corrupt(&self, rng: &mut impl rand::Rng) -> bool {
        self.corruption_rate > 0.0 && rng.gen::<f64>() < self.corruption_rate
    }

    /// Apply corruption to data bytes.
    pub fn corrupt_data(&self, data: &mut [u8], rng: &mut impl rand::Rng) {
        if data.is_empty() {
            return;
        }
        // Flip a random bit in a random byte
        let idx = rng.gen_range(0..data.len());
        let bit = rng.gen_range(0..8);
        data[idx] ^= 1 << bit;
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

    #[test]
    fn test_clock_skew_fault() {
        let mut state = FaultState::new();
        
        // Default skew is 1.0
        assert_eq!(state.clock_skew(1), 1.0);
        
        // Apply 2x clock skew to node 1
        state.apply(&Fault::clock_skew(1, 2.0));
        assert_eq!(state.clock_skew(1), 2.0);
        assert_eq!(state.clock_skew(2), 1.0); // Node 2 unaffected
        
        assert!(state.has_active_faults());
    }

    #[test]
    fn test_clock_jump_fault() {
        let mut state = FaultState::new();
        
        // Default offset is 0
        assert_eq!(state.clock_offset(1), 0);
        
        // Jump node 1 forward by 1 second
        state.apply(&Fault::clock_jump(1, 1_000_000_000));
        assert_eq!(state.clock_offset(1), 1_000_000_000);
        
        // Jumps accumulate
        state.apply(&Fault::clock_jump(1, 500_000_000));
        assert_eq!(state.clock_offset(1), 1_500_000_000);
        
        // Jump backward
        state.apply(&Fault::clock_jump(1, -1_000_000_000));
        assert_eq!(state.clock_offset(1), 500_000_000);
    }

    #[test]
    fn test_adjusted_time() {
        let mut state = FaultState::new();
        
        // No faults - adjusted time equals base time
        assert_eq!(state.adjusted_time(1, 0, 1000), 1000);
        
        // Apply 2x skew
        state.apply(&Fault::clock_skew(1, 2.0));
        // With 2x skew, 1000ns elapsed becomes 2000ns
        assert_eq!(state.adjusted_time(1, 0, 1000), 2000);
        
        // Add a jump offset of 500ns
        state.apply(&Fault::clock_jump(1, 500));
        // 2000 + 500 = 2500
        assert_eq!(state.adjusted_time(1, 0, 1000), 2500);
    }

    #[test]
    fn test_clock_fault_heal() {
        let mut state = FaultState::new();
        
        state.apply(&Fault::clock_skew(1, 2.0));
        state.apply(&Fault::clock_jump(1, 1000));
        
        assert!(state.has_active_faults());
        
        state.apply(&Fault::heal());
        
        assert!(!state.has_active_faults());
        assert_eq!(state.clock_skew(1), 1.0);
        assert_eq!(state.clock_offset(1), 0);
    }

    #[test]
    fn test_clock_fault_constructors() {
        let skew = Fault::clock_skew(1, 1.5);
        assert!(matches!(skew, Fault::ClockSkew { node: 1, rate } if rate == 1.5));
        
        let jump = Fault::clock_jump(2, -1000);
        assert!(matches!(jump, Fault::ClockJump { node: 2, delta: -1000 }));
        
        let jump_fwd = Fault::clock_jump_forward(3, Duration::from_secs(1));
        assert!(matches!(jump_fwd, Fault::ClockJump { node: 3, delta: 1_000_000_000 }));
    }

    #[test]
    fn test_clear_clock_faults() {
        let mut state = FaultState::new();
        
        state.apply(&Fault::clock_skew(1, 2.0));
        state.apply(&Fault::clock_jump(1, 1000));
        
        state.clear_clock_skew(1);
        assert_eq!(state.clock_skew(1), 1.0);
        assert_eq!(state.clock_offset(1), 1000); // Offset still there
        
        state.clear_clock_offset(1);
        assert_eq!(state.clock_offset(1), 0);
    }
}
