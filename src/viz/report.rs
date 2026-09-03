//! HTML report generation for visualization

use super::replay::{generate_replay_html, ReplayController};
use super::sequence::SequenceDiagram;
use super::timeline::{Timeline, TimelineBuilder};
use crate::recording::{Event, Header};
use std::io::Write;

fn escape_html(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());

    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(character),
        }
    }

    escaped
}

/// HTML report configuration
#[derive(Debug, Clone)]
pub struct ReportConfig {
    /// Report title
    pub title: String,
    /// Include timeline view
    pub include_timeline: bool,
    /// Include sequence diagram
    pub include_sequence: bool,
    /// Include statistics
    pub include_stats: bool,
}

impl Default for ReportConfig {
    fn default() -> Self {
        Self {
            title: "Chronos Test Report".to_string(),
            include_timeline: true,
            include_sequence: true,
            include_stats: true,
        }
    }
}

/// HTML report builder
pub struct Report {
    config: ReportConfig,
    header: Option<Header>,
    events: Vec<Event>,
    timeline: Timeline,
    sequence: SequenceDiagram,
}

impl Report {
    /// Create a new report from events
    pub fn from_events(events: &[Event], config: ReportConfig) -> Self {
        let events = events.to_vec();
        let mut timeline_builder = TimelineBuilder::new();
        let mut sequence = SequenceDiagram::new();

        for event in &events {
            timeline_builder.add_event(event);
            sequence.add_event(event);
        }

        Self {
            config,
            header: None,
            events,
            timeline: timeline_builder.build(),
            sequence,
        }
    }

    /// Set the recording header
    pub fn with_header(mut self, header: Header) -> Self {
        self.header = Some(header);
        self
    }

    /// Generate HTML report
    pub fn generate_html(&self) -> String {
        let mut html = String::new();

        html.push_str(&self.html_header());
        html.push_str(&self.html_body());
        html.push_str("</html>");

        html
    }

    /// Write HTML report to a writer
    pub fn write_html<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        writer.write_all(self.generate_html().as_bytes())
    }

    fn html_header(&self) -> String {
        let title = escape_html(&self.config.title);

        format!(
            r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{}</title>
    <style>
        :root {{
            --bg: #1a1a2e;
            --surface: #16213e;
            --primary: #0f3460;
            --accent: #e94560;
            --text: #eee;
            --muted: #888;
        }}
        body {{
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            background: var(--bg);
            color: var(--text);
            margin: 0;
            padding: 20px;
            line-height: 1.6;
        }}
        .container {{
            max-width: 1200px;
            margin: 0 auto;
        }}
        h1, h2 {{ color: var(--accent); }}
        .stats {{
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
            gap: 20px;
            margin: 20px 0;
        }}
        .stat-card {{
            background: var(--surface);
            padding: 20px;
            border-radius: 8px;
            text-align: center;
        }}
        .stat-value {{
            font-size: 2em;
            font-weight: bold;
            color: var(--accent);
        }}
        .stat-label {{
            color: var(--muted);
            font-size: 0.9em;
        }}
        .timeline {{
            background: var(--surface);
            padding: 20px;
            border-radius: 8px;
            margin: 20px 0;
            overflow-x: auto;
        }}
        .timeline-entry {{
            display: flex;
            align-items: center;
            padding: 8px 0;
            border-bottom: 1px solid var(--primary);
            border-left: 4px solid transparent;
            padding-left: 8px;
            scroll-margin: 20px;
        }}
        .timeline-entry.current-event {{
            background: rgba(52, 152, 219, 0.25);
            border-left-color: #3498db;
            box-shadow: inset 0 0 0 1px #3498db;
        }}
        .timeline-entry.failure {{
            background: rgba(233, 69, 96, 0.28);
            border-left-color: var(--accent);
            font-weight: 600;
        }}
        .timeline-entry.current-event.failure {{
            background: rgba(233, 69, 96, 0.45);
            box-shadow: inset 0 0 0 2px var(--accent);
        }}
        .timeline-entry.bookmarked {{
            box-shadow: inset 0 -3px 0 #f1c40f;
        }}
        .timeline-index {{
            width: 55px;
            color: var(--muted);
            font-family: monospace;
            font-size: 0.85em;
        }}
        .timeline-time {{
            width: 100px;
            color: var(--muted);
            font-family: monospace;
            font-size: 0.85em;
        }}
        .timeline-task {{
            width: 80px;
            text-align: center;
        }}
        .timeline-event {{
            flex: 1;
        }}
        .badge {{
            display: inline-block;
            padding: 2px 8px;
            border-radius: 4px;
            font-size: 0.8em;
            margin-right: 8px;
        }}
        .badge.spawn {{ background: #2ecc71; color: #000; }}
        .badge.complete {{ background: #3498db; color: #fff; }}
        .badge.yield {{ background: #9b59b6; color: #fff; }}
        .badge.network {{ background: #f39c12; color: #000; }}
        .badge.schedule {{ background: #1abc9c; color: #000; }}
        .badge.failure {{ background: var(--accent); color: #fff; }}
        .badge.time {{ background: #95a5a6; color: #000; }}
        .badge.random {{ background: #e67e22; color: #fff; }}
        .sequence {{
            background: var(--surface);
            padding: 20px;
            border-radius: 8px;
            margin: 20px 0;
        }}
        .message {{
            display: flex;
            align-items: center;
            padding: 8px 0;
            border-bottom: 1px solid var(--primary);
        }}
        .message-arrow {{
            flex: 1;
            text-align: center;
            color: var(--accent);
        }}
        .failures {{
            background: rgba(233, 69, 96, 0.2);
            border: 1px solid var(--accent);
            padding: 20px;
            border-radius: 8px;
            margin: 20px 0;
        }}
        .failures h2 {{ margin-top: 0; }}
    </style>
</head>
"#,
            title
        )
    }

    fn html_body(&self) -> String {
        let mut body = String::from("<body>\n<div class=\"container\">\n");

        body.push_str(&format!("<h1>{}</h1>\n", escape_html(&self.config.title)));

        if self.config.include_stats {
            body.push_str(&self.html_stats());
        }

        if self.timeline.has_failures() {
            body.push_str(&self.html_failures());
        }

        let replay = ReplayController::new(self.events.clone());
        body.push_str(&generate_replay_html(&replay));

        if self.config.include_timeline {
            body.push_str(&self.html_timeline());
        }

        if self.config.include_sequence && self.sequence.message_count() > 0 {
            body.push_str(&self.html_sequence());
        }

        body.push_str("</div>\n</body>\n");
        body
    }

    fn html_stats(&self) -> String {
        let strategy = self
            .header
            .as_ref()
            .map(|h| match h.strategy {
                0 => "FIFO",
                1 => "Random",
                2 => "PCT",
                _ => "Unknown",
            })
            .unwrap_or("Unknown");

        let seed = self
            .header
            .as_ref()
            .map(|h| h.seed.to_string())
            .unwrap_or_else(|| "N/A".to_string());

        format!(
            r#"<div class="stats">
    <div class="stat-card">
        <div class="stat-value">{}</div>
        <div class="stat-label">Events</div>
    </div>
    <div class="stat-card">
        <div class="stat-value">{}</div>
        <div class="stat-label">Tasks</div>
    </div>
    <div class="stat-card">
        <div class="stat-value">{}</div>
        <div class="stat-label">Messages</div>
    </div>
    <div class="stat-card">
        <div class="stat-value">{}</div>
        <div class="stat-label">Failures</div>
    </div>
    <div class="stat-card">
        <div class="stat-value">{}</div>
        <div class="stat-label">Strategy</div>
    </div>
    <div class="stat-card">
        <div class="stat-value" style="font-size: 1em;">{}</div>
        <div class="stat-label">Seed</div>
    </div>
</div>
"#,
            self.timeline.event_count(),
            self.timeline.task_count(),
            self.sequence.message_count(),
            self.timeline.failure_count(),
            escape_html(strategy),
            escape_html(&seed),
        )
    }

    fn html_failures(&self) -> String {
        let mut html = String::from("<div class=\"failures\" role=\"alert\">\n<h2>Failures</h2>\n");

        for failure in &self.timeline.failures {
            html.push_str(&format!(
                "<div class=\"timeline-entry failure failure-summary\">\
                    <span class=\"timeline-time\">{}ns</span>\
                    <span class=\"badge failure\">{}</span>\
                    <span>{}</span>\
                </div>\n",
                failure.timestamp,
                escape_html(&failure.event_type),
                escape_html(&failure.description),
            ));
        }

        html.push_str("</div>\n");
        html
    }

    fn html_timeline(&self) -> String {
        let mut html =
            String::from("<div id=\"chronos-timeline\" class=\"timeline\">\n<h2>Timeline</h2>\n");

        for (index, entry) in self.timeline.entries.iter().enumerate() {
            let failure_class = if entry.css_class == "failure" {
                " failure"
            } else {
                ""
            };
            let current_class = if index == 0 { " current-event" } else { "" };
            let current_attribute = if index == 0 {
                " aria-current=\"step\""
            } else {
                ""
            };

            html.push_str(&format!(
                "<div class=\"timeline-entry{}{}\" data-event-index=\"{}\" tabindex=\"0\"{} \
                    aria-label=\"Event {}: {}\">\
                    <span class=\"timeline-index\" aria-hidden=\"true\">#{}</span>\
                    <span class=\"timeline-time\">{}ns</span>\
                    <span class=\"timeline-task\">T{}</span>\
                    <span class=\"badge {}\">{}</span>\
                    <span class=\"timeline-event\">{}</span>\
                </div>\n",
                failure_class,
                current_class,
                index,
                current_attribute,
                escape_html(&entry.event_type),
                escape_html(&entry.description),
                index,
                entry.timestamp,
                entry.task_id,
                escape_html(&entry.css_class),
                escape_html(&entry.event_type),
                escape_html(&entry.description),
            ));
        }

        html.push_str("</div>\n");
        html
    }

    fn html_sequence(&self) -> String {
        let mut html = String::from("<div class=\"sequence\">\n<h2>Message Sequence</h2>\n");

        if let Some(latency) = self.sequence.avg_latency_ns() {
            html.push_str(&format!("<p>Average latency: {}ns</p>\n", latency));
        }
        html.push_str(&format!(
            "<p>Total bytes: {}</p>\n",
            self.sequence.total_bytes()
        ));

        let participants = self.sequence.participants();

        for msg in self.sequence.messages() {
            let from_name = participants
                .get(&msg.from)
                .map(|s| s.as_str())
                .unwrap_or("?");
            let to_name = participants.get(&msg.to).map(|s| s.as_str()).unwrap_or("?");

            html.push_str(&format!(
                "<div class=\"message\">\
                    <span>{} (T{})</span>\
                    <span class=\"message-arrow\">--[{} bytes]--></span>\
                    <span>{} (T{})</span>\
                </div>\n",
                escape_html(from_name),
                msg.from,
                msg.size,
                escape_html(to_name),
                msg.to,
            ));
        }

        html.push_str("</div>\n");
        html
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recording::Event;

    #[test]
    fn empty_report() {
        let report = Report::from_events(&[], ReportConfig::default());
        let html = report.generate_html();

        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("Chronos Test Report"));
    }

    #[test]
    fn report_with_events() {
        let events = vec![
            Event::task_spawn(1, 0, "main".to_string(), 0),
            Event::task_complete(1, 1000),
        ];

        let report = Report::from_events(&events, ReportConfig::default());
        let html = report.generate_html();

        assert!(html.contains("main"));
        assert!(html.contains("spawn"));
        assert!(html.contains("complete"));
        assert!(html.contains("data-event-index=\"0\""));
        assert!(html.contains("data-event-index=\"1\""));
        assert!(html.contains("current-event"));
        assert!(html.contains("replay-play-pause"));
    }

    #[test]
    fn report_with_failures() {
        let events = vec![
            Event::task_spawn(1, 0, "main".to_string(), 0),
            Event::fault_injected(0, 500, 1, 1),
        ];

        let report = Report::from_events(&events, ReportConfig::default());
        let html = report.generate_html();

        assert!(html.contains("Failures"));
        assert!(html.contains("failure"));
        assert!(html.contains("data-first-failure=\"1\""));
        assert!(html.contains("timeline-entry failure"));
    }

    #[test]
    fn report_with_messages() {
        let events = vec![
            Event::task_spawn(1, 0, "a".to_string(), 0),
            Event::task_spawn(2, 0, "b".to_string(), 0),
            Event::net_send(1, 100, 2, vec![1, 2, 3]),
            Event::net_recv(2, 150, 1, vec![1, 2, 3]),
        ];

        let report = Report::from_events(&events, ReportConfig::default());
        let html = report.generate_html();

        assert!(html.contains("Message Sequence"));
        assert!(html.contains("3 bytes"));
    }

    #[test]
    fn report_with_header() {
        let events = vec![Event::task_spawn(1, 0, "main".to_string(), 0)];
        let header = Header::new(12345, 1);

        let report = Report::from_events(&events, ReportConfig::default()).with_header(header);
        let html = report.generate_html();

        assert!(html.contains("12345"));
        assert!(html.contains("Random"));
    }

    #[test]
    fn custom_title() {
        let config = ReportConfig {
            title: "My Custom Report".to_string(),
            ..Default::default()
        };

        let report = Report::from_events(&[], config);
        let html = report.generate_html();

        assert!(html.contains("My Custom Report"));
    }

    #[test]
    fn html_escapes_config_and_recording_strings() {
        let hostile_title = r#"<script>alert('title')</script> & "quoted""#;
        let hostile_task_name = r#"<img src=x onerror="alert('task')"> & 'task'"#;
        let config = ReportConfig {
            title: hostile_title.to_string(),
            ..Default::default()
        };
        let events = vec![
            Event::task_spawn(1, 0, hostile_task_name.to_string(), 0),
            Event::fault_injected(0, 100, 1, 1),
        ];

        let html = Report::from_events(&events, config).generate_html();

        assert!(html.contains("&lt;script&gt;alert(&#39;title&#39;)&lt;/script&gt;"));
        assert!(html.contains("&lt;img src=x onerror=&quot;alert(&#39;task&#39;)&quot;&gt;"));
        assert!(!html.contains(hostile_title));
        assert!(!html.contains(hostile_task_name));
    }

    #[test]
    fn write_to_buffer() {
        let events = vec![Event::task_spawn(1, 0, "test".to_string(), 0)];
        let report = Report::from_events(&events, ReportConfig::default());

        let mut buffer = Vec::new();
        report.write_html(&mut buffer).unwrap();

        let html = String::from_utf8(buffer).unwrap();
        assert!(html.contains("test"));
    }
}
