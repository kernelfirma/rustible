use crate::telemetry::config::TelemetryConfig;

#[derive(Debug, Clone)]
pub struct TelemetryLayer;

impl TelemetryLayer {
    pub fn new(_config: &TelemetryConfig) -> Self {
        Self
    }
}

#[derive(Debug, Clone)]
pub struct TelemetryGuard {
    config: TelemetryConfig,
}

impl TelemetryGuard {
    pub fn new(config: TelemetryConfig) -> Self {
        Self { config }
    }

    pub fn shutdown(&self) {}

    pub fn config(&self) -> &TelemetryConfig {
        &self.config
    }
}

#[derive(Debug, Clone)]
pub struct TelemetryBuilder {
    config: TelemetryConfig,
}

impl TelemetryBuilder {
    pub fn new() -> Self {
        Self {
            config: TelemetryConfig::default(),
        }
    }

    pub fn from_config(config: TelemetryConfig) -> Self {
        Self { config }
    }

    pub fn with_service_name(mut self, name: impl Into<String>) -> Self {
        self.config.service_name = name.into();
        self
    }

    pub fn with_otlp_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.config.tracing.otlp_endpoint = Some(endpoint.into());
        self
    }

    pub fn with_prometheus_port(mut self, port: u16) -> Self {
        self.config.metrics.prometheus_port = Some(port);
        self
    }

    pub fn build(self) -> crate::error::Result<TelemetryGuard> {
        Ok(TelemetryGuard::new(self.config))
    }
}
