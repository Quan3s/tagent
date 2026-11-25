//! Token cost estimation. Prices are user-overridable in `[settings.pricing]`;
//! by default cost is reported as 0 rather than inventing a price table.

use crate::provider::Usage;
use std::collections::HashMap;

pub fn estimate(model: &str, usage: Usage, overrides: &HashMap<String, f64>) -> f64 {
    let input_rate = overrides
        .get(&format!("{model}.input"))
        .or_else(|| overrides.get(model))
        .copied()
        .unwrap_or(0.0);
    let output_rate = overrides
        .get(&format!("{model}.output"))
        .copied()
        .unwrap_or(input_rate);
    (usage.input_tokens as f64 * input_rate + usage.output_tokens as f64 * output_rate)
        / 1_000_000.0
}

pub fn today_spend() -> f64 {
    // TODO(verify): persist per-day spend in session metadata (M3 cost panel).
    0.0
}
