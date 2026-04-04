//! Data race detection using happens-before analysis.
//!
//! Tracks memory accesses and synchronization to detect concurrent
//! conflicting accesses.

use std::collections::{HashMap, HashSet};

use crate::cluster::VectorClock;
use crate::{EventId, TaskId};

/// Type of memory access.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AccessType {
    Read,
    Write,
}

/// A memory access event.
#[derive(Debug, Clone)]
pub struct MemoryAccess {
    /// Unique ID for this access.
    pub id: EventId,
    /// Task that performed the access.
    pub task: TaskId,
    /// Memory location (address or variable name hash).
    pub location: u64,
    /// Type of access.
    pub access_type: AccessType,
    /// Vector clock at time of access.
    pub clock: VectorClock,
    /// Source location for reporting.
    pub source_loc: Option<String>,
}

/// A detected data race.
#[derive(Debug, Clone)]
pub struct DataRace {
    /// First access in the race.
    pub access1: MemoryAccess,
    /// Second access in the race.
    pub access2: MemoryAccess,
    /// Description of the race.
    pub description: String,
}

impl DataRace {
    /// Check if this race involves a write.
    pub fn has_write(&self) -> bool {
        self.access1.access_type == AccessType::Write 
            || self.access2.access_type == AccessType::Write
    }

    /// Get the memory location involved.
    pub fn location(&self) -> u64 {
        self.access1.location
    }
}

/// Detector for data races.
#[derive(Debug)]
pub struct RaceDetector {
    /// Recent accesses per memory location.
    /// We keep a bounded history to avoid unbounded memory.
    accesses: HashMap<u64, Vec<MemoryAccess>>,
    /// Maximum history size per location.
    max_history: usize,
    /// Current vector clock per task.
    task_clocks: HashMap<TaskId, VectorClock>,
    /// Next event ID.
    next_id: EventId,
    /// Detected races.
    races: Vec<DataRace>,
    /// Locations to ignore (e.g., known-safe atomics).
    ignore_locations: HashSet<u64>,
}

impl RaceDetector {
    /// Create a new race detector.
    pub fn new() -> Self {
        Self {
            accesses: HashMap::new(),
            max_history: 100,
            task_clocks: HashMap::new(),
            next_id: 0,
            races: Vec::new(),
            ignore_locations: HashSet::new(),
        }
    }

    /// Create with custom history size.
    pub fn with_history_size(max_history: usize) -> Self {
        let mut detector = Self::new();
        detector.max_history = max_history;
        detector
    }

    /// Add a location to ignore (e.g., atomic variables).
    pub fn ignore_location(&mut self, location: u64) {
        self.ignore_locations.insert(location);
    }

    /// Record a synchronization point (e.g., lock acquire).
    /// This creates a happens-before edge from the release to this acquire.
    pub fn synchronize(&mut self, task: TaskId, from_task: TaskId) {
        let from_clock = self.task_clocks.get(&from_task).cloned()
            .unwrap_or_else(VectorClock::new);
        
        let clock = self.task_clocks.entry(task).or_default();
        clock.merge(&from_clock);
    }

    /// Record a memory access.
    pub fn record_access(
        &mut self,
        task: TaskId,
        location: u64,
        access_type: AccessType,
        source_loc: Option<String>,
    ) -> Option<DataRace> {
        // Check if location is ignored
        if self.ignore_locations.contains(&location) {
            return None;
        }

        // Get or create task's vector clock
        let clock = self.task_clocks.entry(task).or_default();
        clock.increment(task);
        let access_clock = clock.clone();

        // Create the access record
        let access = MemoryAccess {
            id: self.next_id,
            task,
            location,
            access_type,
            clock: access_clock.clone(),
            source_loc,
        };
        self.next_id += 1;

        // Check for races against previous accesses
        let mut race = None;
        
        if let Some(previous) = self.accesses.get(&location) {
            for prev_access in previous {
                // Skip same task
                if prev_access.task == task {
                    continue;
                }

                // Check if concurrent (neither happens-before the other)
                let hb1 = prev_access.clock.happened_before(&access_clock);
                let hb2 = access_clock.happened_before(&prev_access.clock);

                if !hb1 && !hb2 {
                    // Concurrent! Check if it's a race (at least one write)
                    if prev_access.access_type == AccessType::Write 
                        || access_type == AccessType::Write 
                    {
                        let description = format!(
                            "Data race on location 0x{:x}: {} by task {} and {} by task {} are concurrent",
                            location,
                            if prev_access.access_type == AccessType::Write { "write" } else { "read" },
                            prev_access.task,
                            if access_type == AccessType::Write { "write" } else { "read" },
                            task,
                        );

                        let data_race = DataRace {
                            access1: prev_access.clone(),
                            access2: access.clone(),
                            description,
                        };

                        self.races.push(data_race.clone());
                        race = Some(data_race);
                        break; // Report first race found
                    }
                }
            }
        }

        // Add this access to history
        let history = self.accesses.entry(location).or_default();
        history.push(access);
        
        // Trim history if needed
        if history.len() > self.max_history {
            history.remove(0);
        }

        race
    }

    /// Record a read access.
    pub fn read(&mut self, task: TaskId, location: u64) -> Option<DataRace> {
        self.record_access(task, location, AccessType::Read, None)
    }

    /// Record a write access.
    pub fn write(&mut self, task: TaskId, location: u64) -> Option<DataRace> {
        self.record_access(task, location, AccessType::Write, None)
    }

    /// Get all detected races.
    pub fn races(&self) -> &[DataRace] {
        &self.races
    }

    /// Get race count.
    pub fn race_count(&self) -> usize {
        self.races.len()
    }

    /// Check if any races were detected.
    pub fn has_races(&self) -> bool {
        !self.races.is_empty()
    }

    /// Clear all state.
    pub fn reset(&mut self) {
        self.accesses.clear();
        self.task_clocks.clear();
        self.races.clear();
        self.next_id = 0;
    }

    /// Get unique race locations.
    pub fn race_locations(&self) -> HashSet<u64> {
        self.races.iter().map(|r| r.location()).collect()
    }
}

impl Default for RaceDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_race_detector_new() {
        let detector = RaceDetector::new();
        assert!(!detector.has_races());
        assert_eq!(detector.race_count(), 0);
    }

    #[test]
    fn test_no_race_same_task() {
        let mut detector = RaceDetector::new();
        
        // Same task reading and writing - no race
        detector.write(1, 100);
        let race = detector.read(1, 100);
        
        assert!(race.is_none());
        assert!(!detector.has_races());
    }

    #[test]
    fn test_no_race_read_read() {
        let mut detector = RaceDetector::new();
        
        // Two reads - no race even if concurrent
        detector.read(1, 100);
        let race = detector.read(2, 100);
        
        assert!(race.is_none());
        assert!(!detector.has_races());
    }

    #[test]
    fn test_race_write_write() {
        let mut detector = RaceDetector::new();
        
        // Two concurrent writes
        detector.write(1, 100);
        let race = detector.write(2, 100);
        
        assert!(race.is_some());
        assert!(detector.has_races());
        assert_eq!(detector.race_count(), 1);
    }

    #[test]
    fn test_race_read_write() {
        let mut detector = RaceDetector::new();
        
        // Concurrent read and write
        detector.read(1, 100);
        let race = detector.write(2, 100);
        
        assert!(race.is_some());
        assert!(detector.has_races());
    }

    #[test]
    fn test_no_race_synchronized() {
        let mut detector = RaceDetector::new();
        
        // Task 1 writes
        detector.write(1, 100);
        
        // Task 2 synchronizes with task 1 (e.g., acquires lock)
        detector.synchronize(2, 1);
        
        // Task 2 reads - no race because of synchronization
        let race = detector.read(2, 100);
        
        assert!(race.is_none());
    }

    #[test]
    fn test_ignore_location() {
        let mut detector = RaceDetector::new();
        detector.ignore_location(100);
        
        // Concurrent writes to ignored location
        detector.write(1, 100);
        let race = detector.write(2, 100);
        
        assert!(race.is_none());
        assert!(!detector.has_races());
    }

    #[test]
    fn test_race_description() {
        let mut detector = RaceDetector::new();
        
        detector.write(1, 0x1234);
        let race = detector.read(2, 0x1234).unwrap();
        
        assert!(race.description.contains("0x1234"));
        assert!(race.description.contains("write"));
        assert!(race.description.contains("read"));
    }

    #[test]
    fn test_multiple_races() {
        let mut detector = RaceDetector::new();
        
        // Race on location 100
        detector.write(1, 100);
        detector.write(2, 100);
        
        // Race on location 200
        detector.write(3, 200);
        detector.write(4, 200);
        
        assert_eq!(detector.race_count(), 2);
        
        let locations = detector.race_locations();
        assert!(locations.contains(&100));
        assert!(locations.contains(&200));
    }

    #[test]
    fn test_reset() {
        let mut detector = RaceDetector::new();
        
        detector.write(1, 100);
        detector.write(2, 100);
        assert!(detector.has_races());
        
        detector.reset();
        assert!(!detector.has_races());
        assert_eq!(detector.race_count(), 0);
    }

    #[test]
    fn test_with_history_size() {
        let detector = RaceDetector::with_history_size(50);
        assert_eq!(detector.max_history, 50);
    }

    #[test]
    fn test_race_has_write() {
        let mut detector = RaceDetector::new();
        
        detector.write(1, 100);
        let race = detector.read(2, 100).unwrap();
        
        assert!(race.has_write());
    }
}
