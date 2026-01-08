use tracing::Level;

pub struct SamplingDecision {
    pub should_log: bool,
    pub sampling_reason: String,
}

pub fn should_sample(
    level: &Level,
    event_name: &str,
    duration_ms: Option<f64>,
) -> SamplingDecision {
    if *level == Level::ERROR || *level == Level::WARN {
        return SamplingDecision {
            should_log: true,
            sampling_reason: "error_or_warn".to_string(),
        };
    }

    if let Some(duration) = duration_ms {
        if duration > 1000.0 {
            return SamplingDecision {
                should_log: true,
                sampling_reason: "slow_operation".to_string(),
            };
        }
    }

    match event_name {
        "playbook_execution" => {
            if rand::random::<f32>() < 0.1 {
                return SamplingDecision {
                    should_log: true,
                    sampling_reason: "random".to_string(),
                };
            }
        }
        "task_execution" => {
            if rand::random::<f32>() < 0.05 {
                return SamplingDecision {
                    should_log: true,
                    sampling_reason: "random".to_string(),
                };
            }
        }
        _ => {}
    }

    SamplingDecision {
        should_log: false,
        sampling_reason: "sampled_out".to_string(),
    }
}
