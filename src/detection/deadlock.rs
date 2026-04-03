use std::collections::{HashMap, HashSet};

use crate::TaskId;

/// A wait graph tracking task dependencies.
/// Edge A -> B means task A is waiting for task B.
#[derive(Debug, Default)]
pub struct WaitGraph {
    /// Adjacency list: task -> set of tasks it's waiting for
    edges: HashMap<TaskId, HashSet<TaskId>>,
}

impl WaitGraph {
    /// Create a new empty wait graph.
    pub fn new() -> Self {
        Self {
            edges: HashMap::new(),
        }
    }

    /// Add a wait edge: `waiter` is now waiting for `holder`.
    pub fn add_wait(&mut self, waiter: TaskId, holder: TaskId) {
        self.edges.entry(waiter).or_default().insert(holder);
    }

    /// Remove a wait edge.
    pub fn remove_wait(&mut self, waiter: TaskId, holder: TaskId) {
        if let Some(holders) = self.edges.get_mut(&waiter) {
            holders.remove(&holder);
            if holders.is_empty() {
                self.edges.remove(&waiter);
            }
        }
    }

    /// Remove all edges for a task (e.g., when task completes).
    pub fn remove_task(&mut self, task: TaskId) {
        self.edges.remove(&task);
        for holders in self.edges.values_mut() {
            holders.remove(&task);
        }
    }

    /// Check if the graph is empty.
    pub fn is_empty(&self) -> bool {
        self.edges.is_empty()
    }

    /// Get the number of edges.
    pub fn edge_count(&self) -> usize {
        self.edges.values().map(|s| s.len()).sum()
    }

    /// Detect a cycle (deadlock) in the wait graph.
    /// Returns the cycle path if found, or None.
    pub fn detect_cycle(&self) -> Option<Vec<TaskId>> {
        let mut visited = HashSet::new();
        let mut rec_stack = HashSet::new();
        let mut path = Vec::new();

        for &start in self.edges.keys() {
            if !visited.contains(&start) {
                if let Some(cycle) = self.dfs_cycle(start, &mut visited, &mut rec_stack, &mut path) {
                    return Some(cycle);
                }
            }
        }

        None
    }

    /// DFS helper for cycle detection.
    fn dfs_cycle(
        &self,
        node: TaskId,
        visited: &mut HashSet<TaskId>,
        rec_stack: &mut HashSet<TaskId>,
        path: &mut Vec<TaskId>,
    ) -> Option<Vec<TaskId>> {
        visited.insert(node);
        rec_stack.insert(node);
        path.push(node);

        if let Some(neighbors) = self.edges.get(&node) {
            for &neighbor in neighbors {
                if !visited.contains(&neighbor) {
                    if let Some(cycle) = self.dfs_cycle(neighbor, visited, rec_stack, path) {
                        return Some(cycle);
                    }
                } else if rec_stack.contains(&neighbor) {
                    // Found a cycle - extract it
                    let cycle_start = path.iter().position(|&n| n == neighbor).unwrap();
                    let mut cycle: Vec<TaskId> = path[cycle_start..].to_vec();
                    cycle.push(neighbor); // Close the cycle
                    return Some(cycle);
                }
            }
        }

        path.pop();
        rec_stack.remove(&node);
        None
    }

    /// Check if adding an edge would create a cycle.
    pub fn would_deadlock(&self, waiter: TaskId, holder: TaskId) -> bool {
        // Check if holder can reach waiter (would create cycle)
        let mut visited = HashSet::new();
        self.can_reach(holder, waiter, &mut visited)
    }

    /// Check if `from` can reach `to` via wait edges.
    fn can_reach(&self, from: TaskId, to: TaskId, visited: &mut HashSet<TaskId>) -> bool {
        if from == to {
            return true;
        }

        if !visited.insert(from) {
            return false;
        }

        if let Some(neighbors) = self.edges.get(&from) {
            for &neighbor in neighbors {
                if self.can_reach(neighbor, to, visited) {
                    return true;
                }
            }
        }

        false
    }
}

/// Deadlock detector that wraps a wait graph.
#[derive(Debug, Default)]
pub struct DeadlockDetector {
    graph: WaitGraph,
}

impl DeadlockDetector {
    /// Create a new deadlock detector.
    pub fn new() -> Self {
        Self {
            graph: WaitGraph::new(),
        }
    }

    /// Record that a task is waiting for another.
    pub fn task_waiting(&mut self, waiter: TaskId, holder: TaskId) {
        self.graph.add_wait(waiter, holder);
    }

    /// Record that a task is no longer waiting.
    pub fn task_released(&mut self, waiter: TaskId, holder: TaskId) {
        self.graph.remove_wait(waiter, holder);
    }

    /// Record that a task has completed.
    pub fn task_completed(&mut self, task: TaskId) {
        self.graph.remove_task(task);
    }

    /// Check for deadlock. Returns the cycle if found.
    pub fn check(&self) -> Option<Vec<TaskId>> {
        self.graph.detect_cycle()
    }

    /// Check if an operation would cause deadlock.
    pub fn would_deadlock(&self, waiter: TaskId, holder: TaskId) -> bool {
        self.graph.would_deadlock(waiter, holder)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wait_graph_empty() {
        let graph = WaitGraph::new();
        assert!(graph.is_empty());
        assert_eq!(graph.edge_count(), 0);
    }

    #[test]
    fn test_wait_graph_add_remove() {
        let mut graph = WaitGraph::new();
        
        graph.add_wait(1, 2);
        assert_eq!(graph.edge_count(), 1);
        
        graph.add_wait(1, 3);
        assert_eq!(graph.edge_count(), 2);
        
        graph.remove_wait(1, 2);
        assert_eq!(graph.edge_count(), 1);
    }

    #[test]
    fn test_wait_graph_remove_task() {
        let mut graph = WaitGraph::new();
        
        graph.add_wait(1, 2);
        graph.add_wait(2, 3);
        graph.add_wait(3, 1);
        
        graph.remove_task(2);
        assert_eq!(graph.edge_count(), 1);
    }

    #[test]
    fn test_no_cycle() {
        let mut graph = WaitGraph::new();
        
        // 1 -> 2 -> 3 (no cycle)
        graph.add_wait(1, 2);
        graph.add_wait(2, 3);
        
        assert!(graph.detect_cycle().is_none());
    }

    #[test]
    fn test_simple_cycle() {
        let mut graph = WaitGraph::new();
        
        // 1 -> 2 -> 1 (cycle)
        graph.add_wait(1, 2);
        graph.add_wait(2, 1);
        
        let cycle = graph.detect_cycle();
        assert!(cycle.is_some());
        let cycle = cycle.unwrap();
        assert!(cycle.contains(&1));
        assert!(cycle.contains(&2));
    }

    #[test]
    fn test_longer_cycle() {
        let mut graph = WaitGraph::new();
        
        // 1 -> 2 -> 3 -> 1 (cycle)
        graph.add_wait(1, 2);
        graph.add_wait(2, 3);
        graph.add_wait(3, 1);
        
        let cycle = graph.detect_cycle();
        assert!(cycle.is_some());
    }

    #[test]
    fn test_would_deadlock() {
        let mut graph = WaitGraph::new();
        
        graph.add_wait(1, 2);
        graph.add_wait(2, 3);
        
        // Adding 3 -> 1 would create cycle
        assert!(graph.would_deadlock(3, 1));
        
        // Adding 3 -> 4 would not
        assert!(!graph.would_deadlock(3, 4));
    }

    #[test]
    fn test_detector_basic() {
        let mut detector = DeadlockDetector::new();
        
        detector.task_waiting(1, 2);
        assert!(detector.check().is_none());
        
        detector.task_waiting(2, 1);
        assert!(detector.check().is_some());
    }

    #[test]
    fn test_detector_release() {
        let mut detector = DeadlockDetector::new();
        
        detector.task_waiting(1, 2);
        detector.task_waiting(2, 1);
        assert!(detector.check().is_some());
        
        detector.task_released(2, 1);
        assert!(detector.check().is_none());
    }

    #[test]
    fn test_detector_completed() {
        let mut detector = DeadlockDetector::new();
        
        detector.task_waiting(1, 2);
        detector.task_waiting(2, 3);
        detector.task_waiting(3, 1);
        
        detector.task_completed(2);
        assert!(detector.check().is_none());
    }
}
