//! JUnit XML output for CI integration.

use std::io::Write;
use std::path::Path;
use std::time::Duration;

use super::run::RunResult;
use super::explore::ExploreResult;

/// A test case in JUnit format.
#[derive(Debug, Clone)]
pub struct TestCase {
    pub name: String,
    pub classname: String,
    pub time_secs: f64,
    pub failure: Option<Failure>,
    pub skipped: bool,
}

/// A test failure.
#[derive(Debug, Clone)]
pub struct Failure {
    pub message: String,
    pub failure_type: String,
    pub content: String,
}

/// A test suite containing multiple test cases.
#[derive(Debug, Clone)]
pub struct TestSuite {
    pub name: String,
    pub tests: u32,
    pub failures: u32,
    pub errors: u32,
    pub skipped: u32,
    pub time_secs: f64,
    pub timestamp: String,
    pub cases: Vec<TestCase>,
}

impl TestSuite {
    /// Create a new test suite.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            tests: 0,
            failures: 0,
            errors: 0,
            skipped: 0,
            time_secs: 0.0,
            timestamp: chrono_timestamp(),
            cases: Vec::new(),
        }
    }

    /// Add a test case.
    pub fn add_case(&mut self, case: TestCase) {
        self.tests += 1;
        self.time_secs += case.time_secs;
        if case.failure.is_some() {
            self.failures += 1;
        }
        if case.skipped {
            self.skipped += 1;
        }
        self.cases.push(case);
    }

    /// Convert to JUnit XML string.
    pub fn to_xml(&self) -> String {
        let mut xml = String::new();
        xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        xml.push_str(&format!(
            "<testsuite name=\"{}\" tests=\"{}\" failures=\"{}\" errors=\"{}\" skipped=\"{}\" time=\"{:.3}\" timestamp=\"{}\">\n",
            xml_escape(&self.name),
            self.tests,
            self.failures,
            self.errors,
            self.skipped,
            self.time_secs,
            self.timestamp,
        ));

        for case in &self.cases {
            xml.push_str(&format!(
                "  <testcase name=\"{}\" classname=\"{}\" time=\"{:.3}\"",
                xml_escape(&case.name),
                xml_escape(&case.classname),
                case.time_secs,
            ));

            if case.failure.is_none() && !case.skipped {
                xml.push_str("/>\n");
            } else {
                xml.push_str(">\n");
                if let Some(ref failure) = case.failure {
                    xml.push_str(&format!(
                        "    <failure message=\"{}\" type=\"{}\">{}</failure>\n",
                        xml_escape(&failure.message),
                        xml_escape(&failure.failure_type),
                        xml_escape(&failure.content),
                    ));
                }
                if case.skipped {
                    xml.push_str("    <skipped/>\n");
                }
                xml.push_str("  </testcase>\n");
            }
        }

        xml.push_str("</testsuite>\n");
        xml
    }

    /// Write to a file.
    pub fn write_to_file(&self, path: &Path) -> std::io::Result<()> {
        let mut file = std::fs::File::create(path)?;
        file.write_all(self.to_xml().as_bytes())
    }
}

/// Create a test suite from a RunResult.
pub fn from_run_result(result: &RunResult, test_name: &str) -> TestSuite {
    let mut suite = TestSuite::new(format!("chronos::{}", test_name));
    
    let failure = if result.bugs_found > 0 {
        let trace_content = result.failure_trace
            .as_ref()
            .map(|t| t.iter().map(|e| e.description.as_str()).collect::<Vec<_>>().join("\n"))
            .unwrap_or_default();
        
        Some(Failure {
            message: result.failure_reason.clone().unwrap_or_else(|| "Test failed".to_string()),
            failure_type: "SimulationFailure".to_string(),
            content: trace_content,
        })
    } else {
        None
    };

    let case = TestCase {
        name: test_name.to_string(),
        classname: "chronos.simulation".to_string(),
        time_secs: result.real_time.as_secs_f64(),
        failure,
        skipped: false,
    };

    suite.add_case(case);
    suite
}

/// Create a test suite from an ExploreResult.
pub fn from_explore_result(result: &ExploreResult, test_name: &str) -> TestSuite {
    let mut suite = TestSuite::new(format!("chronos::explore::{}", test_name));
    
    // Add a case for the overall exploration
    let overall_failure = if !result.bugs_found.is_empty() {
        Some(Failure {
            message: format!("{} bugs found across {} schedules", result.bugs_found.len(), result.schedules_explored),
            failure_type: "ExplorationBugsFound".to_string(),
            content: result.bugs_found
                .iter()
                .map(|b| format!("Bug found with seed {}: {}", b.seed, b.description))
                .collect::<Vec<_>>()
                .join("\n"),
        })
    } else {
        None
    };

    let overall_case = TestCase {
        name: format!("{}_exploration", test_name),
        classname: "chronos.explore".to_string(),
        time_secs: result.elapsed.as_secs_f64(),
        failure: overall_failure,
        skipped: false,
    };
    suite.add_case(overall_case);

    // Add individual cases for each bug found
    for (i, bug) in result.bugs_found.iter().enumerate() {
        let case = TestCase {
            name: format!("{}_bug_{}", test_name, i + 1),
            classname: "chronos.explore".to_string(),
            time_secs: 0.0,
            failure: Some(Failure {
                message: bug.description.clone(),
                failure_type: "SimulationBug".to_string(),
                content: format!(
                    "Seed: {}\nReplay: chronos run {} --seed {}\nTrace:\n{}",
                    bug.seed,
                    test_name,
                    bug.seed,
                    bug.trace.join("\n")
                ),
            }),
            skipped: false,
        };
        suite.add_case(case);
    }

    suite
}

/// XML escape special characters.
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Get current timestamp in ISO 8601 format.
fn chrono_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO);
    let secs = now.as_secs();
    
    // Simple ISO 8601 format without external deps
    let days_since_epoch = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;
    
    // Approximate date calculation (good enough for timestamps)
    let mut year = 1970;
    let mut days = days_since_epoch;
    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }
    
    let days_in_months: [u64; 12] = if is_leap_year(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    
    let mut month = 0;
    for (i, &dim) in days_in_months.iter().enumerate() {
        if days < dim {
            month = i + 1;
            break;
        }
        days -= dim;
    }
    let day = days + 1;
    
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", year, month, day, hours, minutes, seconds)
}

fn is_leap_year(year: u64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::output::TraceEntry;

    #[test]
    fn test_empty_suite() {
        let suite = TestSuite::new("empty");
        let xml = suite.to_xml();
        
        assert!(xml.contains("<?xml version=\"1.0\""));
        assert!(xml.contains("testsuite name=\"empty\""));
        assert!(xml.contains("tests=\"0\""));
        assert!(xml.contains("failures=\"0\""));
    }

    #[test]
    fn test_passing_case() {
        let mut suite = TestSuite::new("test_suite");
        suite.add_case(TestCase {
            name: "test_pass".to_string(),
            classname: "tests".to_string(),
            time_secs: 0.5,
            failure: None,
            skipped: false,
        });

        let xml = suite.to_xml();
        assert!(xml.contains("tests=\"1\""));
        assert!(xml.contains("failures=\"0\""));
        assert!(xml.contains("testcase name=\"test_pass\""));
        assert!(xml.contains("time=\"0.500\""));
    }

    #[test]
    fn test_failing_case() {
        let mut suite = TestSuite::new("test_suite");
        suite.add_case(TestCase {
            name: "test_fail".to_string(),
            classname: "tests".to_string(),
            time_secs: 1.2,
            failure: Some(Failure {
                message: "assertion failed".to_string(),
                failure_type: "AssertionError".to_string(),
                content: "expected true, got false".to_string(),
            }),
            skipped: false,
        });

        let xml = suite.to_xml();
        assert!(xml.contains("tests=\"1\""));
        assert!(xml.contains("failures=\"1\""));
        assert!(xml.contains("<failure message=\"assertion failed\""));
        assert!(xml.contains("expected true, got false"));
    }

    #[test]
    fn test_xml_escaping() {
        let escaped = xml_escape("<test & \"value\">");
        assert_eq!(escaped, "&lt;test &amp; &quot;value&quot;&gt;");
    }

    #[test]
    fn test_from_run_result_pass() {
        let result = RunResult {
            iterations_run: 10,
            bugs_found: 0,
            seed_used: 42,
            schedules_explored: 10,
            simulated_time: Duration::from_secs(5),
            real_time: Duration::from_millis(500),
            failure_trace: None,
            failure_reason: None,
            exit_code: 0,
        };

        let suite = from_run_result(&result, "my_test");
        assert_eq!(suite.tests, 1);
        assert_eq!(suite.failures, 0);
        assert_eq!(suite.cases[0].name, "my_test");
    }

    #[test]
    fn test_from_run_result_fail() {
        let result = RunResult {
            iterations_run: 5,
            bugs_found: 1,
            seed_used: 123,
            schedules_explored: 5,
            simulated_time: Duration::from_secs(2),
            real_time: Duration::from_millis(200),
            failure_trace: Some(vec![
                TraceEntry::new(0, "panic at line 42".to_string()),
            ]),
            failure_reason: Some("deadlock detected".to_string()),
            exit_code: 1,
        };

        let suite = from_run_result(&result, "failing_test");
        assert_eq!(suite.tests, 1);
        assert_eq!(suite.failures, 1);
        assert!(suite.cases[0].failure.is_some());
        assert!(suite.cases[0].failure.as_ref().unwrap().message.contains("deadlock"));
    }

    #[test]
    fn test_timestamp_format() {
        let ts = chrono_timestamp();
        // Should be ISO 8601 format
        assert!(ts.contains("T"));
        assert!(ts.ends_with("Z"));
        assert_eq!(ts.len(), 20);
    }

    #[test]
    fn test_multiple_cases() {
        let mut suite = TestSuite::new("multi");
        suite.add_case(TestCase {
            name: "pass1".to_string(),
            classname: "t".to_string(),
            time_secs: 0.1,
            failure: None,
            skipped: false,
        });
        suite.add_case(TestCase {
            name: "fail1".to_string(),
            classname: "t".to_string(),
            time_secs: 0.2,
            failure: Some(Failure {
                message: "oops".to_string(),
                failure_type: "Error".to_string(),
                content: "details".to_string(),
            }),
            skipped: false,
        });
        suite.add_case(TestCase {
            name: "skip1".to_string(),
            classname: "t".to_string(),
            time_secs: 0.0,
            failure: None,
            skipped: true,
        });

        assert_eq!(suite.tests, 3);
        assert_eq!(suite.failures, 1);
        assert_eq!(suite.skipped, 1);
        assert!((suite.time_secs - 0.3).abs() < 0.001);
    }

    #[test]
    fn test_skipped_case_xml() {
        let mut suite = TestSuite::new("skip_suite");
        suite.add_case(TestCase {
            name: "skipped".to_string(),
            classname: "t".to_string(),
            time_secs: 0.0,
            failure: None,
            skipped: true,
        });

        let xml = suite.to_xml();
        assert!(xml.contains("<skipped/>"));
        assert!(xml.contains("skipped=\"1\""));
    }
}
