//! Analyze recorded executions for debugging.

use std::collections::HashMap;
use std::path::PathBuf;

use clap::Args;

use crate::recording::{Event, EventPayload, RecordingReader};
use crate::Result;

/// Arguments for the analyze command.
#[derive(Args, Debug)]
pub struct AnalyzeArgs {
    /// Path to the recording file.
    pub recording: PathBuf,

    /// Show timeline visualization.
    #[arg(long, short = 't')]
    pub timeline: bool,

    /// Show happens-before graph.
    #[arg(long, short = 'g')]
    pub graph: bool,

    /// Detect data races (if tracking enabled).
    #[arg(long, short = 'r')]
    pub races: bool,

    /// Analyze liveness properties.
    #[arg(long, short = 'l')]
    pub liveness: bool,

    /// Output format (text, json).
    #[arg(long, short = 'o', default_value = "text")]
    pub format: String,

    /// Filter by task ID.
    #[arg(long)]
    pub task: Option<u32>,

    /// Filter by event type.
    #[arg(long, short = 'e')]
    pub event_type: Option<String>,

    /// Verbose output.
    #[arg(long, short = 'v')]
    pub verbose: bool,
}

/// Analysis result.
#[derive(Debug)]
pub struct AnalysisResult {
    /// Recording metadata.
    pub seed: u64,
    pub strategy: u8,
    /// Event statistics.
    pub total_events: usize,
    pub events_by_type: HashMap<String, usize>,
    pub events_by_task: HashMap<u32, usize>,
    /// Timeline entries.
    pub timeline: Vec<TimelineEntry>,
    /// Happens-before edges (from_event, to_event).
    pub hb_edges: Vec<(usize, usize)>,
    /// Concurrent event pairs.
    pub concurrent_pairs: Vec<(usize, usize)>,
    /// Potential issues found.
    pub issues: Vec<AnalysisIssue>,
    /// Max simulated time reached.
    pub max_time_ns: u64,
    /// Detected data races.
    pub races: Vec<PotentialRace>,
    /// Liveness issues.
    pub liveness_issues: Vec<LivenessIssue>,
}

/// A potential data race found during analysis.
#[derive(Debug, Clone)]
pub struct PotentialRace {
    pub event1_idx: usize,
    pub event2_idx: usize,
    pub task1: u32,
    pub task2: u32,
    pub description: String,
}

/// A liveness issue found during analysis.
#[derive(Debug, Clone)]
pub struct LivenessIssue {
    pub issue_type: LivenessIssueType,
    pub task_id: Option<u32>,
    pub description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LivenessIssueType {
    IncompleteTasks,
    UndeliveredMessages,
    PotentialDeadlock,
    PotentialLivelock,
}

/// A timeline entry for visualization.
#[derive(Debug, Clone)]
pub struct TimelineEntry {
    pub timestamp_ns: u64,
    pub task_id: u32,
    pub event_type: String,
    pub description: String,
}

/// An issue found during analysis.
#[derive(Debug, Clone)]
pub struct AnalysisIssue {
    pub severity: IssueSeverity,
    pub description: String,
    pub event_id: Option<usize>,
    pub timestamp_ns: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueSeverity {
    Info,
    Warning,
    Error,
}

impl std::fmt::Display for IssueSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IssueSeverity::Info => write!(f, "INFO"),
            IssueSeverity::Warning => write!(f, "WARN"),
            IssueSeverity::Error => write!(f, "ERROR"),
        }
    }
}

/// Execute the analyze command.
pub fn analyze_command(args: AnalyzeArgs) -> Result<AnalysisResult> {
    let reader = RecordingReader::open(&args.recording)?;
    
    let seed = reader.seed();
    let strategy = reader.strategy();
    
    if args.verbose {
        eprintln!("Analyzing: {:?}", args.recording);
        eprintln!("Seed: {}", seed);
        eprintln!("Strategy: {}", strategy);
    }

    let mut total_events = 0;
    let mut events_by_type: HashMap<String, usize> = HashMap::new();
    let mut events_by_task: HashMap<u32, usize> = HashMap::new();
    let mut timeline = Vec::new();
    let mut issues = Vec::new();
    let mut max_time_ns = 0u64;
    
    // Track state for analysis
    let mut active_tasks: HashMap<u32, bool> = HashMap::new();
    let mut pending_sends: HashMap<u32, Vec<(u64, u32)>> = HashMap::new(); // dst -> [(time, src)]

    for event_result in reader.events() {
        let event = event_result?;
        total_events += 1;
        
        // Apply filters
        if let Some(task_filter) = args.task {
            if event.task_id != task_filter {
                continue;
            }
        }
        
        if let Some(ref type_filter) = args.event_type {
            let event_type_str = format!("{:?}", event.event_type);
            if !event_type_str.to_lowercase().contains(&type_filter.to_lowercase()) {
                continue;
            }
        }
        
        let event_type_str = format!("{:?}", event.event_type);
        *events_by_type.entry(event_type_str.clone()).or_insert(0) += 1;
        *events_by_task.entry(event.task_id).or_insert(0) += 1;
        
        max_time_ns = max_time_ns.max(event.timestamp);
        
        // Build timeline
        let description = describe_event(&event);
        timeline.push(TimelineEntry {
            timestamp_ns: event.timestamp,
            task_id: event.task_id,
            event_type: event_type_str,
            description,
        });
        
        // Analyze for issues
        match &event.payload {
            EventPayload::TaskSpawn { parent, name } => {
                active_tasks.insert(event.task_id, true);
                if args.verbose {
                    eprintln!("  Task {} spawned by {} ({})", event.task_id, parent, name);
                }
            }
            EventPayload::TaskComplete => {
                active_tasks.insert(event.task_id, false);
            }
            EventPayload::NetSend { dst, data: _ } => {
                pending_sends
                    .entry(*dst)
                    .or_default()
                    .push((event.timestamp, event.task_id));
            }
            EventPayload::NetRecv { src, data: _ } => {
                // Check if there's a matching send
                if let Some(sends) = pending_sends.get_mut(src) {
                    if sends.is_empty() {
                        issues.push(AnalysisIssue {
                            severity: IssueSeverity::Warning,
                            description: format!(
                                "Receive from {} without matching send",
                                src
                            ),
                            event_id: Some(total_events),
                            timestamp_ns: Some(event.timestamp),
                        });
                    } else {
                        sends.remove(0);
                    }
                }
            }
            EventPayload::ScheduleDecision { chosen, ready } => {
                if ready.len() > 1 && args.verbose {
                    eprintln!(
                        "  Schedule decision at {}ns: chose {} from {:?}",
                        event.timestamp, chosen, ready
                    );
                }
            }
            EventPayload::FaultInjected { fault_type, target } => {
                issues.push(AnalysisIssue {
                    severity: IssueSeverity::Info,
                    description: format!(
                        "Fault type {} injected at target {}",
                        fault_type, target
                    ),
                    event_id: Some(total_events),
                    timestamp_ns: Some(event.timestamp),
                });
            }
            _ => {}
        }
    }
    
    // Check for incomplete tasks
    for (task_id, active) in &active_tasks {
        if *active {
            issues.push(AnalysisIssue {
                severity: IssueSeverity::Warning,
                description: format!("Task {} never completed", task_id),
                event_id: None,
                timestamp_ns: None,
            });
        }
    }
    
    // Check for undelivered messages
    for (dst, sends) in &pending_sends {
        if !sends.is_empty() {
            issues.push(AnalysisIssue {
                severity: IssueSeverity::Warning,
                description: format!(
                    "{} messages to node {} were never received",
                    sends.len(),
                    dst
                ),
                event_id: None,
                timestamp_ns: None,
            });
        }
    }

    // Build happens-before edges
    let mut hb_edges = Vec::new();
    let mut concurrent_pairs = Vec::new();
    
    // Track last event per task for program order
    let mut last_event_per_task: HashMap<u32, usize> = HashMap::new();
    // Track send events for message ordering
    let mut send_events: HashMap<(u32, u32), Vec<usize>> = HashMap::new(); // (from, to) -> event indices
    
    for (idx, entry) in timeline.iter().enumerate() {
        // Program order: each event happens-after the previous event in same task
        if let Some(&prev_idx) = last_event_per_task.get(&entry.task_id) {
            hb_edges.push((prev_idx, idx));
        }
        last_event_per_task.insert(entry.task_id, idx);
        
        // Message ordering: send happens-before recv
        if entry.event_type.contains("NetSend") {
            // Extract destination from description (crude but works)
            if let Some(dst_start) = entry.description.find("node ") {
                if let Ok(dst) = entry.description[dst_start + 5..].trim().parse::<u32>() {
                    send_events.entry((entry.task_id, dst)).or_default().push(idx);
                }
            }
        } else if entry.event_type.contains("NetRecv") {
            // Find matching send
            if let Some(src_start) = entry.description.find("node ") {
                if let Ok(src) = entry.description[src_start + 5..].trim().parse::<u32>() {
                    if let Some(sends) = send_events.get_mut(&(src, entry.task_id)) {
                        if let Some(send_idx) = sends.first().copied() {
                            hb_edges.push((send_idx, idx));
                            sends.remove(0);
                        }
                    }
                }
            }
        }
    }
    
    // Find concurrent events (events without HB relationship)
    // Build reachability set
    let mut reachable: HashMap<usize, std::collections::HashSet<usize>> = HashMap::new();
    for (from, to) in &hb_edges {
        reachable.entry(*from).or_default().insert(*to);
    }
    // Transitive closure (simplified - for small graphs)
    let n = timeline.len();
    for k in 0..n.min(100) {
        for i in 0..n.min(100) {
            if let Some(i_reaches) = reachable.get(&i).cloned() {
                if i_reaches.contains(&k) {
                    if let Some(k_reaches) = reachable.get(&k).cloned() {
                        reachable.entry(i).or_default().extend(k_reaches);
                    }
                }
            }
        }
    }
    
    // Find concurrent pairs (different tasks, no HB relationship)
    // Track all events per task for liveness analysis
    let task_events_full: HashMap<u32, Vec<usize>> = {
        let mut m: HashMap<u32, Vec<usize>> = HashMap::new();
        for (idx, entry) in timeline.iter().enumerate() {
            m.entry(entry.task_id).or_default().push(idx);
        }
        m
    };
    
    // Limited version for concurrent pair analysis (performance)
    let task_events: HashMap<u32, Vec<usize>> = {
        let mut m: HashMap<u32, Vec<usize>> = HashMap::new();
        for (idx, entry) in timeline.iter().enumerate().take(50) {
            m.entry(entry.task_id).or_default().push(idx);
        }
        m
    };
    
    for (task1, events1) in &task_events {
        for (task2, events2) in &task_events {
            if task1 >= task2 {
                continue;
            }
            for &e1 in events1.iter().take(10) {
                for &e2 in events2.iter().take(10) {
                    let hb_1_2 = reachable.get(&e1).map(|s| s.contains(&e2)).unwrap_or(false);
                    let hb_2_1 = reachable.get(&e2).map(|s| s.contains(&e1)).unwrap_or(false);
                    if !hb_1_2 && !hb_2_1 {
                        concurrent_pairs.push((e1, e2));
                    }
                }
            }
        }
    }

    // Detect potential races from concurrent pairs
    let mut races = Vec::new();
    if args.races {
        for &(e1, e2) in &concurrent_pairs {
            if let (Some(ev1), Some(ev2)) = (timeline.get(e1), timeline.get(e2)) {
                // Check if either event could be a memory access (heuristic based on event types)
                // In a real implementation, we'd have explicit memory access events
                let is_state_modifying_1 = ev1.event_type.contains("TaskYield") 
                    || ev1.event_type.contains("NetSend")
                    || ev1.event_type.contains("NetRecv");
                let is_state_modifying_2 = ev2.event_type.contains("TaskYield")
                    || ev2.event_type.contains("NetSend")
                    || ev2.event_type.contains("NetRecv");
                
                if is_state_modifying_1 && is_state_modifying_2 {
                    races.push(PotentialRace {
                        event1_idx: e1,
                        event2_idx: e2,
                        task1: ev1.task_id,
                        task2: ev2.task_id,
                        description: format!(
                            "Concurrent operations: T{} {} || T{} {}",
                            ev1.task_id, ev1.description,
                            ev2.task_id, ev2.description
                        ),
                    });
                }
            }
        }
    }

    // Analyze liveness
    let mut liveness_issues = Vec::new();
    if args.liveness {
        // Check for incomplete tasks
        let incomplete_count = active_tasks.values().filter(|&&active| active).count();
        if incomplete_count > 0 {
            liveness_issues.push(LivenessIssue {
                issue_type: LivenessIssueType::IncompleteTasks,
                task_id: None,
                description: format!("{} tasks never completed", incomplete_count),
            });
        }

        // Check for undelivered messages
        let undelivered: usize = pending_sends.values().map(|v| v.len()).sum();
        if undelivered > 0 {
            liveness_issues.push(LivenessIssue {
                issue_type: LivenessIssueType::UndeliveredMessages,
                task_id: None,
                description: format!("{} messages were never received", undelivered),
            });
        }

        // Check for potential deadlock patterns (all tasks blocked at end)
        let all_incomplete = active_tasks.values().all(|&active| active);
        if !active_tasks.is_empty() && all_incomplete && incomplete_count > 1 {
            liveness_issues.push(LivenessIssue {
                issue_type: LivenessIssueType::PotentialDeadlock,
                task_id: None,
                description: format!("All {} tasks appear to be blocked", incomplete_count),
            });
        }

        // Check for livelock patterns (tasks with many yields but no completion)
        for (task_id, events) in &task_events_full {
            let yield_count = events.iter()
                .filter(|&&idx| timeline.get(idx).map(|e| e.event_type.contains("Yield")).unwrap_or(false))
                .count();
            let completed = !active_tasks.get(task_id).copied().unwrap_or(true);
            
            if yield_count > 100 && !completed {
                liveness_issues.push(LivenessIssue {
                    issue_type: LivenessIssueType::PotentialLivelock,
                    task_id: Some(*task_id),
                    description: format!("Task {} has {} yields without completing", task_id, yield_count),
                });
            }
        }
    }

    let result = AnalysisResult {
        seed,
        strategy,
        total_events,
        events_by_type,
        events_by_task,
        timeline,
        hb_edges,
        concurrent_pairs,
        issues,
        max_time_ns,
        races,
        liveness_issues,
    };
    
    // Print results based on format
    match args.format.as_str() {
        "json" => print_json(&result, &args)?,
        _ => print_text(&result, &args),
    }
    
    Ok(result)
}

fn describe_event(event: &Event) -> String {
    match &event.payload {
        EventPayload::TaskSpawn { parent, name } => {
            format!("spawn({}) by task {}", name, parent)
        }
        EventPayload::TaskYield => "yield".to_string(),
        EventPayload::TaskComplete => "complete".to_string(),
        EventPayload::TimeQuery { result } => {
            format!("time_query() = {}ns", result)
        }
        EventPayload::RandomGen { result } => {
            format!("random() = {}", result)
        }
        EventPayload::NetSend { dst, data } => {
            format!("send({} bytes) -> node {}", data.len(), dst)
        }
        EventPayload::NetRecv { src, data } => {
            format!("recv({} bytes) <- node {}", data.len(), src)
        }
        EventPayload::ScheduleDecision { chosen, ready } => {
            format!("schedule: chose {} from {:?}", chosen, ready)
        }
        EventPayload::FaultInjected { fault_type, target } => {
            format!("fault(type={}) @ target {}", fault_type, target)
        }
    }
}

fn print_text(result: &AnalysisResult, args: &AnalyzeArgs) {
    println!("=== Chronos Analysis ===");
    println!();
    println!("Recording: {:?}", args.recording);
    println!("Seed: {}", result.seed);
    println!("Strategy: {}", result.strategy);
    println!();
    
    println!("=== Statistics ===");
    println!("Total events: {}", result.total_events);
    println!("Max simulated time: {}ns ({:.3}ms)", 
        result.max_time_ns, 
        result.max_time_ns as f64 / 1_000_000.0
    );
    println!();
    
    println!("Events by type:");
    let mut types: Vec<_> = result.events_by_type.iter().collect();
    types.sort_by(|a, b| b.1.cmp(a.1));
    for (event_type, count) in types {
        println!("  {:20} {}", event_type, count);
    }
    println!();
    
    println!("Events by task:");
    let mut tasks: Vec<_> = result.events_by_task.iter().collect();
    tasks.sort_by_key(|&(id, _)| id);
    for (task_id, count) in tasks {
        println!("  Task {:4}: {} events", task_id, count);
    }
    println!();
    
    if args.timeline && !result.timeline.is_empty() {
        println!("=== Timeline ===");
        for entry in result.timeline.iter().take(100) {
            println!(
                "{:12}ns  task {:3}  {:20}  {}",
                entry.timestamp_ns,
                entry.task_id,
                entry.event_type,
                entry.description
            );
        }
        if result.timeline.len() > 100 {
            println!("  ... ({} more events)", result.timeline.len() - 100);
        }
        println!();
    }
    
    if args.graph {
        println!("=== Happens-Before Graph ===");
        println!();
        
        // Collect tasks
        let mut tasks: Vec<u32> = result.events_by_task.keys().copied().collect();
        tasks.sort();
        
        // Print header
        print!("     ");
        for task in &tasks {
            print!(" T{:<3}", task);
        }
        println!();
        
        // Build per-task event lists
        let mut task_events: HashMap<u32, Vec<(usize, &TimelineEntry)>> = HashMap::new();
        for (idx, entry) in result.timeline.iter().enumerate().take(30) {
            task_events.entry(entry.task_id).or_default().push((idx, entry));
        }
        
        // Print ASCII visualization
        let max_events = task_events.values().map(|v| v.len()).max().unwrap_or(0);
        for row in 0..max_events.min(20) {
            print!("{:>4} ", row);
            for task in &tasks {
                if let Some(events) = task_events.get(task) {
                    if row < events.len() {
                        let (_idx, entry) = &events[row];
                        let symbol = match entry.event_type.as_str() {
                            "TaskSpawn" => "S",
                            "TaskYield" => "Y",
                            "TaskComplete" => "C",
                            s if s.contains("NetSend") => ">",
                            s if s.contains("NetRecv") => "<",
                            _ => "·",
                        };
                        print!(" {:^3} ", symbol);
                    } else {
                        print!("     ");
                    }
                } else {
                    print!("     ");
                }
            }
            println!();
        }
        
        println!();
        println!("Legend: S=spawn, Y=yield, C=complete, >=send, <=recv");
        println!();
        
        // Print HB edges
        println!("Happens-Before Edges ({} total):", result.hb_edges.len());
        for (from, to) in result.hb_edges.iter().take(20) {
            if let (Some(from_e), Some(to_e)) = (result.timeline.get(*from), result.timeline.get(*to)) {
                println!("  {} (T{}) -> {} (T{})",
                    from, from_e.task_id,
                    to, to_e.task_id
                );
            }
        }
        if result.hb_edges.len() > 20 {
            println!("  ... ({} more edges)", result.hb_edges.len() - 20);
        }
        println!();
        
        // Print concurrent pairs
        if !result.concurrent_pairs.is_empty() {
            println!("Concurrent Events ({} pairs):", result.concurrent_pairs.len());
            for (e1, e2) in result.concurrent_pairs.iter().take(10) {
                if let (Some(ev1), Some(ev2)) = (result.timeline.get(*e1), result.timeline.get(*e2)) {
                    println!("  {} (T{}) || {} (T{})", 
                        e1, ev1.task_id,
                        e2, ev2.task_id
                    );
                }
            }
            if result.concurrent_pairs.len() > 10 {
                println!("  ... ({} more pairs)", result.concurrent_pairs.len() - 10);
            }
            println!();
        }
    }
    
    if !result.issues.is_empty() {
        println!("=== Issues ===");
        for issue in &result.issues {
            print!("[{}] {}", issue.severity, issue.description);
            if let Some(ts) = issue.timestamp_ns {
                print!(" (at {}ns)", ts);
            }
            println!();
        }
        println!();
    }

    // Print race detection results
    if args.races && !result.races.is_empty() {
        println!("=== Potential Data Races ===");
        println!("Found {} potential race(s):", result.races.len());
        println!();
        for (i, race) in result.races.iter().enumerate().take(20) {
            println!("  {}. {}", i + 1, race.description);
            if let (Some(ev1), Some(ev2)) = (result.timeline.get(race.event1_idx), result.timeline.get(race.event2_idx)) {
                println!("     Event {}: T{} at {}ns - {}", 
                    race.event1_idx, ev1.task_id, ev1.timestamp_ns, ev1.event_type);
                println!("     Event {}: T{} at {}ns - {}",
                    race.event2_idx, ev2.task_id, ev2.timestamp_ns, ev2.event_type);
            }
            println!();
        }
        if result.races.len() > 20 {
            println!("  ... ({} more races)", result.races.len() - 20);
        }
        println!();
    } else if args.races {
        println!("=== Race Detection ===");
        println!("No potential races detected in {} concurrent event pairs.", result.concurrent_pairs.len());
        println!();
    }

    // Print liveness analysis results
    if args.liveness {
        println!("=== Liveness Analysis ===");
        if result.liveness_issues.is_empty() {
            println!("No liveness issues detected.");
        } else {
            println!("Found {} liveness issue(s):", result.liveness_issues.len());
            println!();
            for issue in &result.liveness_issues {
                let type_str = match issue.issue_type {
                    LivenessIssueType::IncompleteTasks => "INCOMPLETE",
                    LivenessIssueType::UndeliveredMessages => "UNDELIVERED",
                    LivenessIssueType::PotentialDeadlock => "DEADLOCK",
                    LivenessIssueType::PotentialLivelock => "LIVELOCK",
                };
                print!("  [{}] {}", type_str, issue.description);
                if let Some(task) = issue.task_id {
                    print!(" (task {})", task);
                }
                println!();
            }
        }
        println!();
    }
    
    println!("Analysis complete.");
}

fn print_json(result: &AnalysisResult, _args: &AnalyzeArgs) -> Result<()> {
    // Simple JSON output without serde_json dependency
    println!("{{");
    println!("  \"seed\": {},", result.seed);
    println!("  \"strategy\": {},", result.strategy);
    println!("  \"total_events\": {},", result.total_events);
    println!("  \"max_time_ns\": {},", result.max_time_ns);
    println!("  \"events_by_type\": {{");
    let types: Vec<_> = result.events_by_type.iter().collect();
    for (i, (k, v)) in types.iter().enumerate() {
        let comma = if i < types.len() - 1 { "," } else { "" };
        println!("    \"{}\": {}{}", k, v, comma);
    }
    println!("  }},");
    println!("  \"issues_count\": {},", result.issues.len());
    println!("  \"races_count\": {},", result.races.len());
    println!("  \"liveness_issues_count\": {},", result.liveness_issues.len());
    println!("  \"concurrent_pairs_count\": {}", result.concurrent_pairs.len());
    println!("}}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recording::{Event, Header, RecordingWriter};
    use tempfile::tempdir;

    fn create_test_recording(path: &std::path::Path) {
        let header = Header::new(42, 1);
        let mut writer = RecordingWriter::new(path, header).unwrap();
        
        writer.write_event(&Event::task_spawn(1, 0, "main".to_string(), 0)).unwrap();
        writer.write_event(&Event::task_yield(1, 100)).unwrap();
        writer.write_event(&Event::net_send(1, 200, 2, vec![1, 2, 3])).unwrap();
        writer.write_event(&Event::task_complete(1, 300)).unwrap();
        
        writer.finish().unwrap();
    }

    #[test]
    fn test_analyze_basic() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.chrn");
        create_test_recording(&path);
        
        let args = AnalyzeArgs {
            recording: path,
            timeline: false,
            graph: false,
            races: false,
            liveness: false,
            format: "text".to_string(),
            task: None,
            event_type: None,
            verbose: false,
        };
        
        let result = analyze_command(args).unwrap();
        
        assert_eq!(result.seed, 42);
        assert_eq!(result.total_events, 4);
    }

    #[test]
    fn test_analyze_with_filter() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.chrn");
        create_test_recording(&path);
        
        let args = AnalyzeArgs {
            recording: path,
            timeline: false,
            graph: false,
            races: false,
            liveness: false,
            format: "text".to_string(),
            task: Some(1),
            event_type: None,
            verbose: false,
        };
        
        let result = analyze_command(args).unwrap();
        assert_eq!(result.total_events, 4);
    }
}
