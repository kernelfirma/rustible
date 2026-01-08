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

    let fmt_layer = tracing_subscriber::fmt::layer()
        .json()
        .with_span_events(FmtSpan::CLOSE)
        .with_current_span(false)
        .with_span_list(true);

        let subscriber = if let Some(file_path) = log_file {
            Registry::default()
                .with(filter)
                .with_writer(std::fs::File::create(file_path).unwrap())
                .with_ansi(false),
        } else {
            Registry::default()
                .with(filter)
                .with_writer(std::fs::File::create(file_path).unwrap())
                .with_ansi(false),
                .with(fmt_layer)
        };

    subscriber.init();
}
