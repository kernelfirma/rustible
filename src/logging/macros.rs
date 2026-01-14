#[macro_export]
macro_rules! wide_event {
    (
        $event_name:expr,
        $( $key:ident = $value:expr ),* $(,)?
    ) => {
        {
            let decision = crate::logging::should_sample(
                &tracing::Level::INFO,
                $event_name,
                None
            );

            if decision.should_log {
                info!(
                    event_name = $event_name,
                    $( $key = $value, )*
                    sampling_reason = %decision.sampling_reason,
                );
            }
        };
    };
}

#[macro_export]
macro_rules! wide_event_error {
    (
        $event_name:expr,
        $( $key:ident = $value:expr ),* $(,)?
    ) => {
        {
            info!(
                event_name = $event_name,
                event_type = "error",
                $( $key = $value, )*
                sampling_reason = "error",
            );
        };
    };
}
