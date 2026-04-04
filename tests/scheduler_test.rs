//! Tests for the scheduling system.

use chronos::scheduler::{Scheduler, Strategy, ScheduleStrategy, PCTStrategy, DFSStrategy, ContextBoundStrategy};

#[test]
fn test_fifo_scheduler() {
    let mut scheduler = Scheduler::fifo();
    
    // Add tasks
    let t1 = scheduler.add_task();
    let t2 = scheduler.add_task();
    let t3 = scheduler.add_task();
    
    assert_eq!(t1, 0);
    assert_eq!(t2, 1);
    assert_eq!(t3, 2);
    
    // FIFO order
    assert_eq!(scheduler.next(), Some(0));
    assert_eq!(scheduler.next(), Some(1));
    assert_eq!(scheduler.next(), Some(2));
    assert_eq!(scheduler.next(), None);
}

#[test]
fn test_random_scheduler_deterministic() {
    let strategy = Strategy::Random { seed: 12345 };
    let mut s1 = Scheduler::new(strategy.clone());
    let mut s2 = Scheduler::new(strategy);
    
    // Add same tasks to both
    for _ in 0..5 {
        s1.add_task();
        s2.add_task();
    }
    
    // Should produce same sequence with same seed
    for _ in 0..5 {
        assert_eq!(s1.next(), s2.next());
    }
}

#[test]
fn test_pct_strategy_basics() {
    let mut pct = PCTStrategy::new(42, 3);
    
    assert_eq!(pct.seed(), 42);
    assert_eq!(pct.bug_depth(), 3);
    
    // First task seen gets highest priority
    let chosen = pct.select(&[1, 2, 3]);
    assert!(chosen >= 1 && chosen <= 3);
}

#[test]
fn test_pct_strategy_deterministic() {
    let mut pct1 = PCTStrategy::new(99, 2);
    let mut pct2 = PCTStrategy::new(99, 2);
    
    let ready = vec![1, 2, 3, 4, 5];
    
    for _ in 0..20 {
        assert_eq!(pct1.select(&ready), pct2.select(&ready));
    }
}

#[test]
fn test_pct_strategy_reset() {
    let mut pct = PCTStrategy::new(42, 3);
    
    let first_run: Vec<_> = (0..10).map(|_| pct.select(&[1, 2, 3])).collect();
    
    pct.reset();
    
    let second_run: Vec<_> = (0..10).map(|_| pct.select(&[1, 2, 3])).collect();
    
    assert_eq!(first_run, second_run);
}

#[test]
fn test_dfs_strategy() {
    let mut dfs = DFSStrategy::new(100);
    
    // Should select first ready task
    let chosen = dfs.select(&[3, 1, 2]);
    assert_eq!(chosen, 3);
}

#[test]
fn test_context_bound_strategy() {
    let mut cb = ContextBoundStrategy::new(3, 42);
    
    // Should work without crashing
    let chosen = cb.select(&[1, 2, 3]);
    assert!(chosen >= 1 && chosen <= 3);
}

#[test]
fn test_scheduler_block_unblock() {
    let mut scheduler = Scheduler::fifo();
    
    let t1 = scheduler.add_task();
    let t2 = scheduler.add_task();
    
    // Block t1
    scheduler.mark_blocked(t1, chronos::runtime::BlockReason::Channel);
    
    // Only t2 should be ready
    assert_eq!(scheduler.next(), Some(t2));
    assert_eq!(scheduler.next(), None);
    
    // Unblock t1
    scheduler.mark_ready(t1);
    assert_eq!(scheduler.next(), Some(t1));
}

#[test]
fn test_scheduler_remove_task() {
    let mut scheduler = Scheduler::fifo();
    
    let t1 = scheduler.add_task();
    let t2 = scheduler.add_task();
    let t3 = scheduler.add_task();
    
    scheduler.remove_task(t2);
    
    assert_eq!(scheduler.task_count(), 2);
    assert_eq!(scheduler.next(), Some(t1));
    assert_eq!(scheduler.next(), Some(t3));
}

#[test]
fn test_scheduler_all_blocked() {
    let mut scheduler = Scheduler::fifo();
    
    let t1 = scheduler.add_task();
    
    assert!(!scheduler.all_blocked());
    
    scheduler.mark_blocked(t1, chronos::runtime::BlockReason::Channel);
    
    assert!(scheduler.all_blocked());
}

#[test]
fn test_scheduler_reset() {
    let mut scheduler = Scheduler::fifo();
    
    scheduler.add_task();
    scheduler.add_task();
    
    scheduler.reset();
    
    assert_eq!(scheduler.task_count(), 0);
    assert_eq!(scheduler.ready_count(), 0);
}
