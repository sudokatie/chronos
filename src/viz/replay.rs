//! Schedule replay controls for visualization

use crate::recording::{Event, RecordingReader};
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
        let pos = self
            .events
            .iter()
            .position(|e| e.timestamp >= timestamp)
            .unwrap_or(self.events.len());
        self.jump_to(pos);
    }

    /// Jump to first failure event
    pub fn jump_to_first_failure(&mut self) -> bool {
        use crate::recording::EventType;
        if let Some(pos) = self
            .events
            .iter()
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
        if start >= end {
            return &[];
        }

        let start = start.min(self.events.len());
        let end = end.min(self.events.len());

        if start >= end {
            return &[];
        }

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
    let total_events = controller.total_events();
    let progress_pct = (controller.progress() * 100.0).round() as u32;
    let first_failure = controller
        .all_events()
        .iter()
        .position(|event| event.event_type == crate::recording::EventType::FaultInjected);
    let speed = match controller.speed().0 {
        0.5 => 0.5,
        1.0 => 1.0,
        2.0 => 2.0,
        4.0 => 4.0,
        _ => 1.0,
    };
    let selected_speed = |option: f64| if speed == option { " selected" } else { "" };
    let is_playing = controller.state() == ReplayState::Playing;
    let bookmarks = controller
        .bookmarks()
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let bookmark_count = controller.bookmarks().len();
    let bookmark_label = if bookmark_count == 1 {
        "bookmark"
    } else {
        "bookmarks"
    };
    let initial_play_icon = if is_playing { "⏸" } else { "▶" };
    let initial_play_label = if is_playing { "Pause" } else { "Play" };
    let initial_play_title = if is_playing {
        "Pause replay"
    } else {
        "Play replay"
    };
    let initial_status = match controller.state() {
        ReplayState::Playing => "Playing",
        ReplayState::Paused => "Paused",
        ReplayState::Finished => "Finished",
        ReplayState::Stopped => "Stopped",
    };
    let initial_bookmark_action = if controller.bookmarks().contains(&controller.position()) {
        "Remove bookmark"
    } else {
        "Add bookmark"
    };

    let mut html = String::new();
    html.push_str("<section id=\"chronos-replay-controls\" class=\"replay-controls\" ");
    html.push_str("role=\"group\" aria-label=\"Schedule replay controls\" ");
    html.push_str(&format!(
        "data-event-count=\"{}\" data-position=\"{}\" data-playing=\"{}\" ",
        total_events,
        controller.position(),
        is_playing,
    ));
    html.push_str("data-first-failure=\"");
    if let Some(first_failure) = first_failure {
        html.push_str(&first_failure.to_string());
    }
    html.push_str(&format!(
        "\" data-speed=\"{}\" data-bookmarks=\"{}\">\n",
        speed, bookmarks
    ));
    html.push_str(
        &r#"    <div class="replay-buttons">
        <button type="button" id="replay-stop" title="Stop replay" aria-label="Stop replay">
            <span aria-hidden="true">⏹</span> <span>Stop</span>
        </button>
        <button type="button" id="replay-step-back" title="Previous event" aria-label="Previous event">
            <span aria-hidden="true">⏮</span> <span>Previous</span>
        </button>
        <button type="button" id="replay-play-pause" title="__PLAY_TITLE__" aria-label="__PLAY_TITLE__">
            <span aria-hidden="true">__PLAY_ICON__</span> <span class="play-label">__PLAY_LABEL__</span>
        </button>
        <button type="button" id="replay-step-fwd" title="Next event" aria-label="Next event">
            <span aria-hidden="true">⏭</span> <span>Next</span>
        </button>
        <button type="button" id="replay-jump-failure" title="Jump to first failure" aria-label="Jump to first failure">
            <span aria-hidden="true">⚠</span> <span>First failure</span>
        </button>
    </div>
    <div class="replay-progress">
        <label for="replay-slider">Event position</label>
        <input type="range" id="replay-slider" min="0" max=""#
            .replace("__PLAY_TITLE__", initial_play_title)
            .replace("__PLAY_ICON__", initial_play_icon)
            .replace("__PLAY_LABEL__", initial_play_label),
    );
    html.push_str(&total_events.to_string());
    let position = controller.position().to_string();
    let total = total_events.to_string();
    let progress = progress_pct.to_string();
    html.push_str(
        &r#"" value="__POSITION__" step="1" aria-label="Seek replay event" aria-valuemin="0" aria-valuemax="__TOTAL__" aria-valuenow="__POSITION__" />
        <span id="replay-position" class="replay-position" role="status" aria-live="polite">__POSITION__ / __TOTAL__ (__PROGRESS__%)</span>
    </div>
    <div class="replay-speed">
        <label for="replay-speed">Playback speed</label>
        <select id="replay-speed" aria-label="Playback speed">
            <option value="0.5"__SLOW_SELECTED__>0.5x</option>
            <option value="1"__NORMAL_SELECTED__>1x</option>
            <option value="2"__FAST_SELECTED__>2x</option>
            <option value="4"__VERY_FAST_SELECTED__>4x</option>
        </select>
    </div>
    <div class="replay-bookmarks">
        <button type="button" id="replay-add-bookmark" title="__BOOKMARK_ACTION__" aria-label="__BOOKMARK_ACTION__">
            <span aria-hidden="true">🔖</span> <span class="bookmark-action">__BOOKMARK_ACTION__</span>
        </button>
        <button type="button" id="replay-prev-bookmark" title="Previous bookmark" aria-label="Previous bookmark">
            <span aria-hidden="true">◀🔖</span> <span>Previous bookmark</span>
        </button>
        <button type="button" id="replay-next-bookmark" title="Next bookmark" aria-label="Next bookmark">
            <span aria-hidden="true">🔖▶</span> <span>Next bookmark</span>
        </button>
        <span id="replay-bookmark-count" class="bookmark-count" role="status" aria-live="polite">__BOOKMARK_COUNT__ __BOOKMARK_LABEL__</span>
    </div>
    <span id="replay-status" class="replay-status" role="status" aria-live="polite">__INITIAL_STATUS__</span>
</section>
<style>
.replay-controls {
    background: var(--surface);
    padding: 15px;
    border-radius: 8px;
    margin: 20px 0;
    display: flex;
    flex-wrap: wrap;
    gap: 20px;
    align-items: center;
}
.replay-buttons,
.replay-bookmarks,
.replay-speed {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 8px;
}
.replay-buttons button,
.replay-bookmarks button,
.replay-speed select {
    background: var(--primary);
    color: var(--text);
    border: 1px solid transparent;
    padding: 8px 12px;
    border-radius: 4px;
    cursor: pointer;
    font: inherit;
}
.replay-buttons button:hover,
.replay-bookmarks button:hover,
.replay-speed select:hover {
    background: var(--accent);
}
.replay-buttons button:focus-visible,
.replay-bookmarks button:focus-visible,
.replay-speed select:focus-visible,
.replay-progress input:focus-visible {
    outline: 3px solid #fff;
    outline-offset: 2px;
}
.replay-buttons button:disabled,
.replay-bookmarks button:disabled {
    cursor: not-allowed;
    opacity: 0.5;
}
.replay-progress {
    flex: 1;
    min-width: 220px;
}
.replay-progress label,
.replay-speed label {
    display: block;
    color: var(--muted);
    font-size: 0.9em;
}
.replay-progress input[type="range"] {
    width: 100%;
}
.replay-position,
.replay-status {
    display: block;
    color: var(--muted);
    font-size: 0.9em;
}
.replay-position {
    text-align: center;
}
.bookmark-count {
    color: var(--muted);
    font-size: 0.9em;
}
</style>
<script>
(function () {
    function initReplayControls() {
        const controls = document.getElementById("chronos-replay-controls");
        if (!controls) {
            return;
        }

        const stopButton = document.getElementById("replay-stop");
        const previousButton = document.getElementById("replay-step-back");
        const playPauseButton = document.getElementById("replay-play-pause");
        const nextButton = document.getElementById("replay-step-fwd");
        const failureButton = document.getElementById("replay-jump-failure");
        const slider = document.getElementById("replay-slider");
        const positionLabel = document.getElementById("replay-position");
        const speedSelect = document.getElementById("replay-speed");
        const addBookmarkButton = document.getElementById("replay-add-bookmark");
        const previousBookmarkButton = document.getElementById("replay-prev-bookmark");
        const nextBookmarkButton = document.getElementById("replay-next-bookmark");
        const bookmarkCount = document.getElementById("replay-bookmark-count");
        const status = document.getElementById("replay-status");
        const playLabel = playPauseButton.querySelector(".play-label");
        const bookmarkAction = addBookmarkButton.querySelector(".bookmark-action");
        const rows = Array.from(document.querySelectorAll(".timeline-entry[data-event-index]"));
        const eventCount = Math.max(0, Number(controls.dataset.eventCount) || 0);
        const firstFailureValue = controls.dataset.firstFailure;
        const firstFailure = firstFailureValue === "" ? null : Number(firstFailureValue);
        let position = Math.min(eventCount, Math.max(0, Number(controls.dataset.position) || 0));
        let speed = Number(controls.dataset.speed) || 1;
        let playing = controls.dataset.playing === "true" && position < eventCount;
        let timer = null;
        const bookmarks = new Set((controls.dataset.bookmarks || "").split(",").filter(Boolean).map(Number));

        function stopTimer() {
            if (timer !== null) {
                window.clearInterval(timer);
                timer = null;
            }
        }

        function startTimer() {
            stopTimer();
            timer = window.setInterval(advance, 500 / speed);
        }

        function setPlaying(value) {
            playing = value && position < eventCount;
            if (playing) {
                startTimer();
            } else {
                stopTimer();
            }
            update();
        }

        function advance() {
            if (position >= eventCount) {
                setPlaying(false);
                return;
            }
            position += 1;
            if (position >= eventCount) {
                playing = false;
                stopTimer();
            }
            update();
        }

        function hasBookmarkBefore() {
            return Array.from(bookmarks).some(function (bookmark) {
                return bookmark < position;
            });
        }

        function hasBookmarkAfter() {
            return Array.from(bookmarks).some(function (bookmark) {
                return bookmark > position;
            });
        }

        function update() {
            const percent = eventCount === 0 ? 0 : Math.round(position / eventCount * 100);
            const currentBookmark = bookmarks.has(position);
            rows.forEach(function (row) {
                const index = Number(row.dataset.eventIndex);
                const isCurrent = index === position && position < eventCount;
                row.classList.toggle("current-event", isCurrent);
                row.classList.toggle("bookmarked", bookmarks.has(index));
                if (isCurrent) {
                    row.setAttribute("aria-current", "step");
                } else {
                    row.removeAttribute("aria-current");
                }
                if (bookmarks.has(index)) {
                    row.setAttribute("data-bookmarked", "true");
                } else {
                    row.removeAttribute("data-bookmarked");
                }
            });

            slider.value = String(position);
            slider.setAttribute("aria-valuenow", String(position));
            positionLabel.textContent = position + " / " + eventCount + " (" + percent + "%)";
            playLabel.textContent = playing ? "Pause" : "Play";
            playPauseButton.setAttribute("aria-label", playing ? "Pause replay" : "Play replay");
            playPauseButton.title = playing ? "Pause replay" : "Play replay";
            stopButton.disabled = position === 0 && !playing;
            previousButton.disabled = position === 0;
            nextButton.disabled = position >= eventCount;
            failureButton.disabled = firstFailure === null || firstFailure >= eventCount;
            previousBookmarkButton.disabled = !hasBookmarkBefore();
            nextBookmarkButton.disabled = !hasBookmarkAfter();
            bookmarkAction.textContent = currentBookmark ? "Remove bookmark" : "Add bookmark";
            addBookmarkButton.setAttribute("aria-label", currentBookmark ? "Remove bookmark" : "Add bookmark");
            addBookmarkButton.title = currentBookmark ? "Remove bookmark" : "Add bookmark";
            bookmarkCount.textContent = bookmarks.size + (bookmarks.size === 1 ? " bookmark" : " bookmarks");
            status.textContent = playing ? "Playing" : (position >= eventCount && eventCount > 0 ? "Finished" : (position === 0 ? "Stopped" : "Paused"));
        }

        stopButton.addEventListener("click", function () {
            position = 0;
            setPlaying(false);
        });
        previousButton.addEventListener("click", function () {
            setPlaying(false);
            position = Math.max(0, position - 1);
            update();
        });
        playPauseButton.addEventListener("click", function () {
            if (playing) {
                setPlaying(false);
            } else if (position < eventCount) {
                setPlaying(true);
            }
        });
        nextButton.addEventListener("click", function () {
            setPlaying(false);
            advance();
        });
        failureButton.addEventListener("click", function () {
            if (firstFailure !== null && firstFailure < eventCount) {
                setPlaying(false);
                position = firstFailure;
                update();
            }
        });
        slider.addEventListener("input", function () {
            setPlaying(false);
            position = Math.min(eventCount, Math.max(0, Number(slider.value) || 0));
            update();
        });
        speedSelect.addEventListener("change", function () {
            speed = Number(speedSelect.value) || 1;
            if (playing) {
                startTimer();
            }
        });
        addBookmarkButton.addEventListener("click", function () {
            if (bookmarks.has(position)) {
                bookmarks.delete(position);
            } else {
                bookmarks.add(position);
            }
            update();
        });
        previousBookmarkButton.addEventListener("click", function () {
            const previous = Array.from(bookmarks).filter(function (bookmark) {
                return bookmark < position;
            }).sort(function (a, b) {
                return b - a;
            })[0];
            if (previous !== undefined) {
                setPlaying(false);
                position = previous;
                update();
            }
        });
        nextBookmarkButton.addEventListener("click", function () {
            const next = Array.from(bookmarks).filter(function (bookmark) {
                return bookmark > position;
            }).sort(function (a, b) {
                return a - b;
            })[0];
            if (next !== undefined) {
                setPlaying(false);
                position = next;
                update();
            }
        });

        speedSelect.value = String(speed);
        update();
        if (playing) {
            startTimer();
        }
    }

    if (document.readyState === "loading") {
        document.addEventListener("DOMContentLoaded", initReplayControls);
    } else {
        initReplayControls();
    }
}());
</script>
"#
            .replace("__POSITION__", &position)
            .replace("__TOTAL__", &total)
            .replace("__PROGRESS__", &progress)
            .replace("__SLOW_SELECTED__", selected_speed(0.5))
            .replace("__NORMAL_SELECTED__", selected_speed(1.0))
            .replace("__FAST_SELECTED__", selected_speed(2.0))
            .replace("__VERY_FAST_SELECTED__", selected_speed(4.0))
            .replace("__BOOKMARK_COUNT__", &bookmark_count.to_string())
            .replace("__BOOKMARK_LABEL__", bookmark_label)
            .replace("__INITIAL_STATUS__", initial_status)
            .replace("__BOOKMARK_ACTION__", initial_bookmark_action),
    );
    html
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
    fn inverted_events_in_range_is_empty() {
        let ctrl = ReplayController::new(sample_events());

        assert!(ctrl.events_in_range(3, 1).is_empty());
        assert!(ctrl.events_in_range(10, 1).is_empty());
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
        assert!(html.contains("max=\"4\""));
        assert!(html.contains("aria-label=\"Schedule replay controls\""));
        assert!(html.contains("initReplayControls"));
        assert!(html.contains("replay-add-bookmark"));
        assert!(!html.contains("__"));
    }

    #[test]
    fn generate_html_preserves_bookmark_and_play_state() {
        let mut ctrl = ReplayController::new(sample_events());
        ctrl.jump_to(1);
        ctrl.add_bookmark();
        ctrl.play();

        let html = generate_replay_html(&ctrl);

        assert!(html.contains("data-position=\"1\""));
        assert!(html.contains("data-bookmarks=\"1\""));
        assert!(html.contains("1 bookmark"));
        assert!(html.contains("⏸"));
        assert!(html.contains("Pause"));
    }
}
