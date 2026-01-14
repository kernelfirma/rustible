pub mod events;
pub mod init;
pub mod macros;
pub mod sampling;

#[cfg(test)]
mod tests;

pub use events::*;
pub use init::init_logging;
pub use sampling::{should_sample, SamplingDecision};

pub use tracing::{debug, error, info, instrument, span, warn};
