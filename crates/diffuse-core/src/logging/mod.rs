//! Structured Tokio Tracing & Logging Infrastructure.
//!
//! Provides compact hierarchical span formatting, multi-target log dispatching
//! (file, Unix domain socket, TCP stream, or stderr), and zero-overhead
//! filtering.

use std::{env, fs::File, io, sync::Arc};

use tracing_subscriber::{
    EnvFilter,
    filter::LevelFilter,
    fmt::{FormatFields, FormattedFields, format::Writer, writer::BoxMakeWriter},
    layer::SubscriberExt,
    registry::{LookupSpan, Registry},
    util::SubscriberInitExt,
};

/// Initialize global tracing subscriber for structured log events and spans.
///
/// Reads destination from `DIFFUSE_LOG_FILE` (or `RUST_LOG_FILE` fallback):
/// - `tcp:<addr>`: connects to TCP socket
/// - `unix:<path>`: connects to Unix domain socket
/// - `<path>`: appends to specified file
/// - unset: logs to `io::stderr`
///
/// Default log filter is `LevelFilter::OFF` unless overridden by `DIFFUSE_LOG`
/// or `RUST_LOG`.
pub fn init_tracing_subscriber() -> io::Result<()> {
    let env_filter = if let Ok(val) = env::var("DIFFUSE_LOG") {
        EnvFilter::builder()
            .with_default_directive(LevelFilter::OFF.into())
            .parse_lossy(val)
    } else if let Ok(val) = env::var("RUST_LOG") {
        EnvFilter::builder()
            .with_default_directive(LevelFilter::OFF.into())
            .parse_lossy(val)
    } else {
        EnvFilter::builder()
            .with_default_directive(LevelFilter::OFF.into())
            .from_env_lossy()
    };

    let log_target = env::var("DIFFUSE_LOG_FILE").or_else(|_| env::var("RUST_LOG_FILE"));

    let writer = if let Ok(log_path) = log_target {
        if let Some(tcp_addr) = log_path.strip_prefix("tcp:") {
            use std::net::TcpStream;
            let stream = TcpStream::connect(tcp_addr)?;
            BoxMakeWriter::new(Arc::new(stream))
        } else if let Some(socket_path) = log_path.strip_prefix("unix:") {
            use std::os::unix::net::UnixStream;
            let stream = UnixStream::connect(socket_path)?;
            BoxMakeWriter::new(Arc::new(stream))
        } else {
            let file = File::options().create(true).append(true).open(&log_path)?;
            BoxMakeWriter::new(Arc::new(file))
        }
    } else {
        BoxMakeWriter::new(Arc::new(io::stderr()))
    };

    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_writer(writer)
        .with_ansi(false)
        .without_time()
        .with_target(false)
        .with_span_events(
            tracing_subscriber::fmt::format::FmtSpan::ENTER
                | tracing_subscriber::fmt::format::FmtSpan::CLOSE,
        )
        .fmt_fields(LogFieldFormatter)
        .event_format(LogFormat {
            max_head_spans: 2,
            max_tail_spans: 2,
        });

    let _ = Registry::default()
        .with(env_filter)
        .with(fmt_layer)
        .try_init();

    Ok(())
}

/// Custom log event formatter rendering compact file paths and truncated span
/// chains.
pub struct LogFormat {
    pub max_head_spans: usize,
    pub max_tail_spans: usize,
}

impl<S, N> tracing_subscriber::fmt::FormatEvent<S, N> for LogFormat
where
    S: tracing::Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &tracing_subscriber::fmt::FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &tracing::Event<'_>,
    ) -> std::fmt::Result {
        let meta = event.metadata();

        if let Some(file) = meta.file() {
            let parts: Vec<&str> = file.rsplit('/').take(2).collect();
            let display = parts.iter().rev().copied().collect::<Vec<_>>().join("/");
            write!(writer, "{}", display)?;
            if let Some(line) = meta.line() {
                write!(writer, ":{}", line)?;
            }
            write!(writer, ": ")?;
        }

        write!(writer, "{} ", meta.level())?;

        if let Some(scope) = ctx.event_scope() {
            let spans: Vec<_> = scope.from_root().collect();
            let has_spans = !spans.is_empty();

            if spans.len() <= self.max_head_spans + self.max_tail_spans {
                for span in spans {
                    write!(writer, "{}", span.name())?;
                    if let Some(fields) = span.extensions().get::<FormattedFields<N>>()
                        && !fields.is_empty()
                    {
                        write!(writer, "[{}]", fields.as_str())?;
                    }
                    write!(writer, ":")?;
                }
            } else {
                for span in &spans[0..self.max_head_spans] {
                    write!(writer, "{}", span.name())?;
                    if let Some(fields) = span.extensions().get::<FormattedFields<N>>()
                        && !fields.is_empty()
                    {
                        write!(writer, "[{}]", fields.as_str())?;
                    }
                    write!(writer, ":")?;
                }

                if self.max_head_spans > 0 || self.max_tail_spans > 0 {
                    write!(writer, "...:")?;
                }

                for span in &spans[spans.len() - self.max_tail_spans..] {
                    write!(writer, "{}", span.name())?;
                    if let Some(fields) = span.extensions().get::<FormattedFields<N>>()
                        && !fields.is_empty()
                    {
                        write!(writer, "[{}]", fields.as_str())?;
                    }
                    write!(writer, ":")?;
                }
            }
            if has_spans {
                write!(writer, " ")?;
            }
        }

        ctx.format_fields(writer.by_ref(), event)?;
        writeln!(writer)
    }
}

/// Formatter for fields within spans and events.
pub struct LogFieldFormatter;

impl<'writer> tracing_subscriber::fmt::FormatFields<'writer> for LogFieldFormatter {
    fn format_fields<R: tracing_subscriber::field::RecordFields>(
        &self,
        writer: Writer<'writer>,
        fields: R,
    ) -> std::fmt::Result {
        let mut visitor = LogFieldVisitor {
            writer,
            first: true,
        };
        fields.record(&mut visitor);
        Ok(())
    }
}

struct LogFieldVisitor<'writer> {
    writer: Writer<'writer>,
    first: bool,
}

impl<'writer> tracing::field::Visit for LogFieldVisitor<'writer> {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if !self.first {
            let _ = write!(self.writer, ", ");
        }
        self.first = false;

        if field.name() == "message" {
            let msg = match value {
                "enter" => "▶",
                "close" => "◀",
                other => other,
            };
            let _ = write!(self.writer, "{}", msg);
        } else if field.name() == "return" {
            let _ = write!(self.writer, "return: {}", value);
        } else {
            let _ = write!(self.writer, "{}:{}", field.name(), value);
        }
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if !self.first {
            let _ = write!(self.writer, ", ");
        }
        self.first = false;

        let value_str = format!("{:?}", value);
        if field.name() == "message" {
            let display_str = value_str
                .strip_prefix('"')
                .and_then(|s| s.strip_suffix('"'))
                .unwrap_or(&value_str);
            let msg = match display_str {
                "enter" => "▶",
                "close" => "◀",
                other => other,
            };
            let _ = write!(self.writer, "{}", msg);
        } else if field.name() == "return" {
            let _ = write!(self.writer, "return: {}", value_str);
        } else {
            let _ = write!(self.writer, "{}:{}", field.name(), value_str);
        }
    }
}

#[cfg(test)]
mod tests;
