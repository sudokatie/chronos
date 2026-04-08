//! Schedule replay controls for visualization

use crate::recording::{Event, EventIterator, RecordingReader};
use std::path::Path;

/// Replay state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayState {
    /// Replay is stopped at the beginning
    Stopped,
    /// Replay is playing forward
    Playing,
    /// Replay is paused at current position
    Paused,
    /// Replay has reached the end
    Finished,
}

/// Replay speed multiplier
#[derive(Debug, Clone, Copy)]
pub struct ReplaySpeed(pub f64);

impl Default for ReplaySpeed {
    fn default() -> Self {
        Self(1.0)
    }
}

impl ReplaySpeed {
    pub const SLOW: Self = Self(0.5);
    pub const NORMAL: Self = Self(1.0);
    pub const FAST: Self = Self(2.0);
    pub const VERY_FAST: Self = Self(4.0);
}

/// Replay controller for stepping through recorded events
pub struct ReplayController {
    /// All events in the recording
    events: Vec<Event>,
    /// Current position in the event list
    position: usize,
    /// Current replay state
    state: ReplayState,
    /// Playback speed
    speed: ReplaySpeed,
    /// Bookmarked positions
    bookmarks: Vec<usize>,
}

impl ReplayController {
    /// Create a new replay controller from events
    pub fn new(events: Vec<Event>) -> Self {
        Self {
            events,
            position: 0,
            state: ReplayState::Stopped,
            speed: ReplaySpeed::default(),
            bookmarks: Vec::new(),
        }
    }

    /// Load replay from a recording file
    pub fn from_file<P: AsRef<Path>>(path: P) -> crate::Result<Self> {
        let reader = RecordingReader::open(path)?;
        let events: Vec<Event> = reader.events().collect::<Result<Vec<_>, _>>()?;
        Ok(Self::new(events))
    }

    /// Get current state
    pub fn state(&self) -> ReplayState {
        self.state
    }

    /// Get current position
    pub fn position(&self) -> usize {
        self.position
    }

    /// Get total event count
    pub fn total_events(&self) -> usize {
        self.events.len()
    }

    /// Get current event (if any)
    pub fn current_event(&self) -> Option<&Event> {
        self.events.get(self.position)
    }

    /// Get progress as percentage (0.0 to 1.0)
    pub fn progress(&self) -> f64 {
        if self.events.is_empty() {
            return 0.0;
        }
        self.position as f64 / self.events.len() as f64
    }

    /// Start or resume playback
    pub fn play(&mut self) {
        if self.position >= self.events.len() {
            self.state = ReplayState::Finished;
        } else {
            self.state = ReplayState::Playing;
        }
    }

    /// Pause playback
    pub fn pause(&mut self) {
        if self.state == ReplayState::Playing {
            self.state = ReplayState::Paused;
        }
    }

    /// Stop and reset to beginning
    pub fn stop(&mut self) {
        self.position = 0;
        self.state = ReplayState::Stopped;
    }

    /// Step forward one event
    pub fn step_forward(&mut self) -> Option<&Event> {
        if self.position < self.events.len() {
            let event = &self.events[self.position];
            self.position += 1;
            if self.position >= self.events.len() {
                self.state = ReplayState::Finished;
            } else {
                self.state = ReplayState::Paused;
            }
            Some(event)
        } else {
            None
        }
    }

    /// Step backward one event
    pub fn step_backward(&mut self) -> Option<&Event> {
        if self.position > 0 {
            self.position -= 1;
            self.state = ReplayState::Paused;
            Some(&self.events[self.position])
        } else {
            None
        }
    }

    /// Jump to a specific position
    pub fn jump_to(&mut self, position: usize) {
        self.position = position.min(self.events.len());
        if self.position >= self.events.len() {
            self.state = ReplayState::Finished;
        } else {
            self.state = ReplayState::Paused;
        }
    }

    /// Jump to a specific timestamp
    pub fn jump_to_time(&mut self, timestamp: u64) {
        let pos = self.events.iter()
            .position(|e| e.timestamp >= timestamp)
            .unwrap_or(self.events.len());
        self.jump_to(pos);
    }

    /// Jump to first failure event
    pub fn jump_to_first_failure(&mut self) -> bool {
        use crate::recording::EventType;
        if let Some(pos) = self.events.iter()
            .position(|e| e.event_type == EventType::FaultInjected)
        {
            self.jump_to(pos);
            true
        } else {
            false
        }
    }

    /// Set playback speed
    pub fn set_speed(&mut self, speed: ReplaySpeed) {
        self.speed = speed;
    }

    /// Get playback speed
    pub fn speed(&self) -> ReplaySpeed {
        self.speed
    }

    /// Add a bookmark at current position
    pub fn add_bookmark(&mut self) {
        if !self.bookmarks.contains(&self.position) {
            self.bookmarks.push(self.position);
            self.bookmarks.sort();
        }
    }

    /// Remove bookmark at current position
    pub fn remove_bookmark(&mut self) {
        self.bookmarks.retain(|&b| b != self.position);
    }

    /// Get all bookmarks
    pub fn bookmarks(&self) -> &[usize] {
        &self.bookmarks
    }

    /// Jump to next bookmark
    pub fn jump_to_next_bookmark(&mut self) -> bool {
        if let Some(&pos) = self.bookmarks.iter().find(|&&b| b > self.position) {
            self.jump_to(pos);
            true
        } else {
            false
        }
    }

    /// Jump to previous bookmark
    pub fn jump_to_prev_bookmark(&mut self) -> bool {
        if let Some(&pos) = self.bookmarks.iter().rev().find(|&&b| b < self.position) {
            self.jump_to(pos);
            true
        } else {
            false
        }
    }

    /// Get events in a range
    pub fn events_in_range(&self, start: usize, end: usize) -> &[Event] {
        let start = start.min(self.events.len());
        let end = end.min(self.events.len());
        &self.events[start..end]
    }

    /// Get all events
    pub fn all_events(&self) -> &[Event] {
        &self.events
    }

    /// Check if at beginning
    pub fn is_at_start(&self) -> bool {
        self.position == 0
    }

    /// Check if at end
    pub fn is_at_end(&self) -> bool {
        self.position >= self.events.len()
    }
}

/// Generate HTML for replay controls
pub fn generate_replay_html(controller: &ReplayController) -> String {
    let progress_pct = (controller.progress() * 100.0) as u32;
    
    format!(r#"
<div class="replay-controls">
    <div class="replay-buttons">
        <button id="replay-stop" title="Stop">⏹</button>
        <button id="replay-step-back" title="Step Back">⏮</button>
        <button id="replay-play-pause" title="Play/Pause">{}</button>
        <button id="replay-step-fwd" title="Step Forward">⏭</button>
        <button id="replay-jump-failure" title="Jump to Failure">⚠</button>
    </div>
    <div class="replay-progress">
        <input type="range" id="replay-slider" min="0" max="{}" value="{}" />
        <span class="replay-position">{} / {} ({}%)</span>
    </div>
    <div class="replay-speed">
        <label>Speed:</label>
        <select id="replay-speed">
            <option value="0.5">0.5x</option>
            <option value="1" selected>1x</option>
            <option value="2">2x</option>
            <option value="4">4x</option>
        </select>
    </div>
    <div class="replay-bookmarks">
        <button id="replay-add-bookmark" title="Add Bookmark">🔖+</button>
        <button id="replay-prev-bookmark" title="Previous Bookmark">◀🔖</button>
        <button id="replay-next-bookmark" title="Next Bookmark">🔖▶</button>
        <span class="bookmark-count">{} bookmarks</span>
    </div>
</div>
<style>
.replay-controls {{
    background: var(--surface);
    padding: 15px;
    border-radius: 8px;
    margin: 20px 0;
    display: flex;
    flex-wrap: wrap;
    gap: 20px;
    align-items: center;
}}
.replay-buttons button {{
    background: var(--primary);
    color: var(--text);
    border: none;
    padding: 8px 12px;
    border-radius: 4px;
    cursor: pointer;
    font-size: 1.2em;
}}
.replay-buttons button:hover {{
    background: var(--accent);
}}
.replay-progress {{
    flex: 1;
    min-width: 200px;
}}
.replay-progress input[type="range"] {{
    width: 100%;
}}
.replay-position {{
    display: block;
    text-align: center;
    color: var(--muted);
    font-size: 0.9em;
}}
.replay-speed select {{
    background: var(--primary);
    color: var(--text);
    border: none;
    padding: 5px;
    border-radius: 4px;
}}
.replay-bookmarks button {{
    background: var(--primary);
    color: var(--text);
    border: none;
    padding: 5px 10px;
    border-radius: 4px;
    cursor: pointer;
}}
.bookmark-count {{
    color: var(--muted);
    font-size: 0.9em;
}}
</style>
"#,
        if controller.state() == ReplayState::Playing { "⏸" } else { "▶" },
        controller.total_events().saturating_sub(1),
        controller.position(),
        controller.position(),
        controller.total_events(),
        progress_pct,
        controller.bookmarks().len(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recording::Event;

    fn sample_events() -> Vec<Event> {
        vec![
            Event::task_spawn(1, 0, "main".to_string(), 0),
            Event::task_yield(1, 100),
            Event::task_yield(1, 200),
            Event::task_complete(1, 300),
        ]
    }

    #[test]
    fn new_controller() {
        let ctrl = ReplayController::new(sample_events());
        assert_eq!(ctrl.state(), ReplayState::Stopped);
        assert_eq!(ctrl.position(), 0);
        assert_eq!(ctrl.total_events(), 4);
    }

    #[test]
    fn step_forward() {
        let mut ctrl = ReplayController::new(sample_events());
        
        let event = ctrl.step_forward().unwrap();
        assert_eq!(event.task_id, 1);
        assert_eq!(ctrl.position(), 1);
        assert_eq!(ctrl.state(), ReplayState::Paused);
    }

    #[test]
    fn step_backward() {
        let mut ctrl = ReplayController::new(sample_events());
        ctrl.jump_to(2);
        
        let _event = ctrl.step_backward().unwrap();
        assert_eq!(ctrl.position(), 1);
    }

    #[test]
    fn play_pause() {
        let mut ctrl = ReplayController::new(sample_events());
        
        ctrl.play();
        assert_eq!(ctrl.state(), ReplayState::Playing);
        
        ctrl.pause();
        assert_eq!(ctrl.state(), ReplayState::Paused);
    }

    #[test]
    fn stop_resets() {
        let mut ctrl = ReplayController::new(sample_events());
        ctrl.jump_to(2);
        
        ctrl.stop();
        assert_eq!(ctrl.position(), 0);
        assert_eq!(ctrl.state(), ReplayState::Stopped);
    }

    #[test]
    fn jump_to_position() {
        let mut ctrl = ReplayController::new(sample_events());
        
        ctrl.jump_to(2);
        assert_eq!(ctrl.position(), 2);
        assert_eq!(ctrl.state(), ReplayState::Paused);
    }

    #[test]
    fn jump_to_time() {
        let mut ctrl = ReplayController::new(sample_events());
        
        ctrl.jump_to_time(150);
        assert_eq!(ctrl.position(), 2); // First event with timestamp >= 150 is at index 2 (timestamp 200)
    }

    #[test]
    fn jump_beyond_end() {
        let mut ctrl = ReplayController::new(sample_events());
        
        ctrl.jump_to(100);
        assert_eq!(ctrl.position(), 4);
        assert_eq!(ctrl.state(), ReplayState::Finished);
    }

    #[test]
    fn progress_calculation() {
        let mut ctrl = ReplayController::new(sample_events());
        
        assert_eq!(ctrl.progress(), 0.0);
        
        ctrl.jump_to(2);
        assert!((ctrl.progress() - 0.5).abs() < 0.01);
    }

    #[test]
    fn bookmarks() {
        let mut ctrl = ReplayController::new(sample_events());
        
        ctrl.jump_to(1);
        ctrl.add_bookmark();
        ctrl.jump_to(3);
        ctrl.add_bookmark();
        
        assert_eq!(ctrl.bookmarks().len(), 2);
        assert_eq!(ctrl.bookmarks(), &[1, 3]);
    }

    #[test]
    fn navigate_bookmarks() {
        let mut ctrl = ReplayController::new(sample_events());
        
        ctrl.add_bookmark(); // at 0
        ctrl.jump_to(2);
        ctrl.add_bookmark(); // at 2
        
        ctrl.jump_to(1);
        assert!(ctrl.jump_to_next_bookmark());
        assert_eq!(ctrl.position(), 2);
        
        assert!(ctrl.jump_to_prev_bookmark());
        assert_eq!(ctrl.position(), 0);
    }

    #[test]
    fn remove_bookmark() {
        let mut ctrl = ReplayController::new(sample_events());
        
        ctrl.add_bookmark();
        assert_eq!(ctrl.bookmarks().len(), 1);
        
        ctrl.remove_bookmark();
        assert_eq!(ctrl.bookmarks().len(), 0);
    }

    #[test]
    fn speed_setting() {
        let mut ctrl = ReplayController::new(sample_events());
        
        ctrl.set_speed(ReplaySpeed::FAST);
        assert!((ctrl.speed().0 - 2.0).abs() < 0.01);
    }

    #[test]
    fn current_event() {
        let mut ctrl = ReplayController::new(sample_events());
        
        let event = ctrl.current_event().unwrap();
        assert_eq!(event.timestamp, 0);
        
        ctrl.step_forward();
        let event = ctrl.current_event().unwrap();
        assert_eq!(event.timestamp, 100);
    }

    #[test]
    fn events_in_range() {
        let ctrl = ReplayController::new(sample_events());
        
        let events = ctrl.events_in_range(1, 3);
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn is_at_boundaries() {
        let mut ctrl = ReplayController::new(sample_events());
        
        assert!(ctrl.is_at_start());
        assert!(!ctrl.is_at_end());
        
        ctrl.jump_to(4);
        assert!(!ctrl.is_at_start());
        assert!(ctrl.is_at_end());
    }

    #[test]
    fn empty_controller() {
        let ctrl = ReplayController::new(vec![]);
        
        assert_eq!(ctrl.total_events(), 0);
        assert_eq!(ctrl.progress(), 0.0);
        assert!(ctrl.current_event().is_none());
    }

    #[test]
    fn generate_html_output() {
        let ctrl = ReplayController::new(sample_events());
        let html = generate_replay_html(&ctrl);
        
        assert!(html.contains("replay-controls"));
        assert!(html.contains("replay-slider"));
        assert!(html.contains("0 / 4"));
    }
}
