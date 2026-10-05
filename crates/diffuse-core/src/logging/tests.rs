use std::{
    fs,
    io::{self, Write},
    sync::{Arc, Mutex, MutexGuard},
};

use tracing::{info, info_span};
use tracing_subscriber::fmt::MakeWriter;

use super::*;

#[derive(Clone)]
struct SharedWriter(Arc<Mutex<Vec<u8>>>);

struct SharedWriterGuard<'a>(MutexGuard<'a, Vec<u8>>);

impl<'a> Write for SharedWriterGuard<'a> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

impl<'a> MakeWriter<'a> for SharedWriter {
    type Writer = SharedWriterGuard<'a>;

    fn make_writer(&'a self) -> Self::Writer {
        SharedWriterGuard(self.0.lock().unwrap())
    }
}

#[derive(Debug)]
#[allow(dead_code)]
struct DummyMetrics {
    stat: String,
    mmap: String,
}

#[test]
fn test_log_format_no_spaces_in_structs() {
    let buf = Arc::new(Mutex::new(Vec::new()));
    let writer = SharedWriter(buf.clone());

    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_writer(writer)
        .with_ansi(false)
        .without_time()
        .with_target(false)
        .fmt_fields(LogFieldFormatter)
        .event_format(LogFormat {
            max_head_spans: 2,
            max_tail_spans: 2,
        });

    let subscriber = Registry::default().with(fmt_layer);

    tracing::subscriber::with_default(subscriber, || {
        let metrics = DummyMetrics {
            stat: "1.2ms".to_string(),
            mmap: "40us".to_string(),
        };

        let span = info_span!("test_span", metrics = ?metrics, id = 42);
        let _guard = span.enter();

        info!(test_val = 123, "this is a test");
    });

    let output_bytes = buf.lock().unwrap().clone();
    let output = String::from_utf8_lossy(&output_bytes);

    assert!(
        output.contains(
            "test_span[metrics:DummyMetrics { stat: \"1.2ms\", mmap: \"40us\" }, id:42]: "
        ),
        "Span formatting is incorrect. Output was: {}",
        output
    );

    assert!(
        output.contains("test_val:123"),
        "Event formatting is incorrect. Output was: {}",
        output
    );

    assert!(
        output.contains("this is a test"),
        "Message is missing. Output was: {}",
        output
    );

    assert!(
        !output.contains("message:"),
        "Output still contains 'message:' prefix: {}",
        output
    );
}

#[test]
fn test_log_format_nested_spans() {
    let buf = Arc::new(Mutex::new(Vec::new()));
    let writer = SharedWriter(buf.clone());

    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_writer(writer)
        .with_ansi(false)
        .without_time()
        .with_target(false)
        .fmt_fields(LogFieldFormatter)
        .event_format(LogFormat {
            max_head_spans: 2,
            max_tail_spans: 2,
        });

    let subscriber = Registry::default().with(fmt_layer);

    tracing::subscriber::with_default(subscriber, || {
        let outer_span = info_span!("outer_span", outer_id = 1);
        let _outer_guard = outer_span.enter();

        let inner_span = info_span!("inner_span", inner_id = 2);
        let _inner_guard = inner_span.enter();

        info!(test_val = 456, "nested message");
    });

    let output_bytes = buf.lock().unwrap().clone();
    let output = String::from_utf8_lossy(&output_bytes);

    assert!(
        output.contains(
            "outer_span[outer_id:1]:inner_span[inner_id:2]: nested message, test_val:456"
        ),
        "Nested span formatting is incorrect. Output was: {}",
        output
    );
}

#[test]
fn test_log_format_empty_span_fields() {
    let buf = Arc::new(Mutex::new(Vec::new()));
    let writer = SharedWriter(buf.clone());

    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_writer(writer)
        .with_ansi(false)
        .without_time()
        .with_target(false)
        .fmt_fields(LogFieldFormatter)
        .event_format(LogFormat {
            max_head_spans: 2,
            max_tail_spans: 2,
        });

    let subscriber = Registry::default().with(fmt_layer);

    tracing::subscriber::with_default(subscriber, || {
        let span = info_span!("empty_span");
        let _guard = span.enter();

        info!("message with empty span");
    });

    let output_bytes = buf.lock().unwrap().clone();
    let output = String::from_utf8_lossy(&output_bytes);

    assert!(
        output.contains("empty_span: message with empty span"),
        "Empty span formatting is incorrect (should not contain []). Output was: {}",
        output
    );
}

#[test]
fn test_log_format_file_line_level() {
    let buf = Arc::new(Mutex::new(Vec::new()));
    let writer = SharedWriter(buf.clone());

    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_writer(writer)
        .with_ansi(false)
        .without_time()
        .with_target(false)
        .fmt_fields(LogFieldFormatter)
        .event_format(LogFormat {
            max_head_spans: 2,
            max_tail_spans: 2,
        });

    let subscriber = Registry::default().with(fmt_layer);

    tracing::subscriber::with_default(subscriber, || {
        info!("just checking file and line");
    });

    let output_bytes = buf.lock().unwrap().clone();
    let output = String::from_utf8_lossy(&output_bytes);

    assert!(
        output.contains("logging/tests.rs:"),
        "Missing file name formatting. Output was: {}",
        output
    );
    assert!(
        output.contains(": INFO just checking file and line"),
        "Missing log level or message formatting. Output was: {}",
        output
    );
}

#[test]
fn test_log_format_truncated_spans() {
    let buf = Arc::new(Mutex::new(Vec::new()));
    let writer = SharedWriter(buf.clone());

    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_writer(writer)
        .with_ansi(false)
        .without_time()
        .with_target(false)
        .fmt_fields(LogFieldFormatter)
        .event_format(LogFormat {
            max_head_spans: 2,
            max_tail_spans: 2,
        });

    let subscriber = Registry::default().with(fmt_layer);

    tracing::subscriber::with_default(subscriber, || {
        let span1 = info_span!("span1");
        let _g1 = span1.enter();
        let span2 = info_span!("span2");
        let _g2 = span2.enter();
        let span3 = info_span!("span3");
        let _g3 = span3.enter();
        let span4 = info_span!("span4");
        let _g4 = span4.enter();
        let span5 = info_span!("span5");
        let _g5 = span5.enter();

        info!("deeply nested message");
    });

    let output_bytes = buf.lock().unwrap().clone();
    let output = String::from_utf8_lossy(&output_bytes);

    assert!(
        output.contains("span1:span2:...:span4:span5: deeply nested message"),
        "Truncated span formatting is incorrect. Output was: {}",
        output
    );
}

#[test]
fn test_log_format_truncated_spans_zero_head() {
    let buf = Arc::new(Mutex::new(Vec::new()));
    let writer = SharedWriter(buf.clone());

    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_writer(writer)
        .with_ansi(false)
        .without_time()
        .with_target(false)
        .fmt_fields(LogFieldFormatter)
        .event_format(LogFormat {
            max_head_spans: 0,
            max_tail_spans: 2,
        });

    let subscriber = Registry::default().with(fmt_layer);

    tracing::subscriber::with_default(subscriber, || {
        let span1 = info_span!("span1");
        let _g1 = span1.enter();
        let span2 = info_span!("span2");
        let _g2 = span2.enter();
        let span3 = info_span!("span3");
        let _g3 = span3.enter();

        info!("message");
    });

    let output_bytes = buf.lock().unwrap().clone();
    let output = String::from_utf8_lossy(&output_bytes);

    assert!(
        output.contains("...:span2:span3: message"),
        "Zero head span formatting is incorrect. Output was: {}",
        output
    );
}

#[test]
fn test_log_format_truncated_spans_zero_tail() {
    let buf = Arc::new(Mutex::new(Vec::new()));
    let writer = SharedWriter(buf.clone());

    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_writer(writer)
        .with_ansi(false)
        .without_time()
        .with_target(false)
        .fmt_fields(LogFieldFormatter)
        .event_format(LogFormat {
            max_head_spans: 2,
            max_tail_spans: 0,
        });

    let subscriber = Registry::default().with(fmt_layer);

    tracing::subscriber::with_default(subscriber, || {
        let span1 = info_span!("span1");
        let _g1 = span1.enter();
        let span2 = info_span!("span2");
        let _g2 = span2.enter();
        let span3 = info_span!("span3");
        let _g3 = span3.enter();

        info!("message");
    });

    let output_bytes = buf.lock().unwrap().clone();
    let output = String::from_utf8_lossy(&output_bytes);

    assert!(
        output.contains("span1:span2:...: message"),
        "Zero tail span formatting is incorrect. Output was: {}",
        output
    );
}

#[test]
fn test_init_tracing_subscriber_file_output() {
    let temp_file = tempfile::NamedTempFile::new().unwrap();
    let path = temp_file.path().to_str().unwrap().to_string();

    let file = File::options()
        .create(true)
        .append(true)
        .open(&path)
        .unwrap();
    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_writer(Arc::new(file))
        .with_ansi(false)
        .without_time()
        .with_target(false)
        .fmt_fields(LogFieldFormatter)
        .event_format(LogFormat {
            max_head_spans: 2,
            max_tail_spans: 2,
        });

    let subscriber = Registry::default().with(fmt_layer);
    tracing::subscriber::with_default(subscriber, || {
        info!("test event to file");
    });

    let content = fs::read_to_string(&path).unwrap();
    assert!(
        content.contains("test event to file"),
        "Got content: '{content}'"
    );
}

#[test]
fn test_log_format_unquoted_messages_and_return_spacing() {
    let buf = Arc::new(Mutex::new(Vec::new()));
    let writer = SharedWriter(buf.clone());

    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_writer(writer)
        .with_ansi(false)
        .without_time()
        .with_target(false)
        .fmt_fields(LogFieldFormatter)
        .event_format(LogFormat {
            max_head_spans: 2,
            max_tail_spans: 2,
        });

    let subscriber = Registry::default().with(fmt_layer);

    tracing::subscriber::with_default(subscriber, || {
        info!("enter");
        info!("close");
        info!(r#return = "ok", "completed");
    });

    let output_bytes = buf.lock().unwrap().clone();
    let output = String::from_utf8_lossy(&output_bytes);

    // enter and close mapped to unicode symbols without quotes
    assert!(output.contains(": INFO ▶\n"), "Output was: {output}");
    assert!(output.contains(": INFO ◀\n"), "Output was: {output}");
    assert!(
        output.contains("completed, return: ok"),
        "Output was: {output}"
    );
}

#[test]
fn test_default_log_path_macos() {
    let path = default_log_path_for(
        "macos",
        |var| {
            if var == "HOME" {
                Some("/Users/testuser".into())
            } else {
                None
            }
        },
        |_| false,
    )
    .unwrap();
    assert_eq!(
        path,
        PathBuf::from("/Users/testuser/Library/Logs/diffuse/diffuse.log")
    );
}

#[test]
fn test_default_log_path_windows_localappdata() {
    let path = default_log_path_for(
        "windows",
        |var| {
            if var == "LOCALAPPDATA" {
                Some("C:/Users/testuser/AppData/Local".into())
            } else {
                None
            }
        },
        |_| false,
    )
    .unwrap();
    assert_eq!(
        path,
        PathBuf::from("C:/Users/testuser/AppData/Local")
            .join("diffuse")
            .join("diffuse.log")
    );
}

#[test]
fn test_default_log_path_windows_userprofile_fallback() {
    let path = default_log_path_for(
        "windows",
        |var| {
            if var == "USERPROFILE" {
                Some("C:/Users/testuser".into())
            } else {
                None
            }
        },
        |_| false,
    )
    .unwrap();
    assert_eq!(
        path,
        PathBuf::from("C:/Users/testuser")
            .join("AppData")
            .join("Local")
            .join("diffuse")
            .join("diffuse.log")
    );
}

#[test]
fn test_default_log_path_linux_xdg_state_home() {
    let path = default_log_path_for(
        "linux",
        |var| {
            if var == "XDG_STATE_HOME" {
                Some("/custom/state".into())
            } else {
                None
            }
        },
        |_| false,
    )
    .unwrap();
    assert_eq!(path, PathBuf::from("/custom/state/diffuse/diffuse.log"));
}

#[test]
fn test_default_log_path_linux_xdg_data_home() {
    let path = default_log_path_for(
        "linux",
        |var| {
            if var == "XDG_DATA_HOME" {
                Some("/custom/share".into())
            } else {
                None
            }
        },
        |_| false,
    )
    .unwrap();
    assert_eq!(path, PathBuf::from("/custom/share/diffuse/diffuse.log"));
}

#[test]
fn test_default_log_path_linux_share_exists() {
    let path = default_log_path_for(
        "linux",
        |var| {
            if var == "HOME" {
                Some("/home/testuser".into())
            } else {
                None
            }
        },
        |p| p == Path::new("/home/testuser/.share"),
    )
    .unwrap();
    assert_eq!(
        path,
        PathBuf::from("/home/testuser/.share/diffuse/diffuse.log")
    );
}

#[test]
fn test_default_log_path_linux_local_state_default() {
    let path = default_log_path_for(
        "linux",
        |var| {
            if var == "HOME" {
                Some("/home/testuser".into())
            } else {
                None
            }
        },
        |p| p == Path::new("/home/testuser/.local/state"),
    )
    .unwrap();
    assert_eq!(
        path,
        PathBuf::from("/home/testuser/.local/state/diffuse/diffuse.log")
    );
}

#[test]
fn test_default_log_path_none_when_no_env() {
    assert!(default_log_path_for("linux", |_| None, |_| false).is_none());
    assert!(default_log_path_for("macos", |_| None, |_| false).is_none());
    assert!(default_log_path_for("windows", |_| None, |_| false).is_none());
}
