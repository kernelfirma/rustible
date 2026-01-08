use std::env;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Registry};

pub fn init_logging() {
    let filter = env::var("RUST_LOG")
        .unwrap_or_else(|_| "rustible=info".to_string())
        .parse::<EnvFilter>()
        .unwrap_or_else(|_| EnvFilter::new("rustible=info"));

    let subscriber = Registry::default().with(filter).with(
        tracing_subscriber::fmt::layer()
            .json()
            .with_span_events(FmtSpan::CLOSE)
            .with_current_span(false)
            .with_span_list(true),
    );

    subscriber.init();
}

pub fn init_logging_with_file(log_file: Option<&str>) {
    let filter = env::var("RUST_LOG")
        .unwrap_or_else(|_| "rustible=info".to_string())
        .parse::<EnvFilter>()
        .unwrap_or_else(|_| EnvFilter::new("rustible=info"));

    if let Some(file_path) = log_file {
        // Log to file with JSON format, no ANSI colors
        let file = std::fs::File::create(file_path).expect("Failed to create log file");
        let fmt_layer = tracing_subscriber::fmt::layer()
            .json()
            .with_span_events(FmtSpan::CLOSE)
            .with_current_span(false)
            .with_span_list(true)
            .with_ansi(false)
            .with_writer(file);

        Registry::default().with(filter).with(fmt_layer).init();
    } else {
        // Log to stdout with JSON format
        let fmt_layer = tracing_subscriber::fmt::layer()
            .json()
            .with_span_events(FmtSpan::CLOSE)
            .with_current_span(false)
            .with_span_list(true);

        Registry::default().with(filter).with(fmt_layer).init();
    }
}
