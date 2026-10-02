//! How detector scores are shown (detectionplan.md Phase 1).
//!
//! Rotation, debris and Tornado ID scores are evidence scores: how much radar evidence there is,
//! on a 0..1 scale, not calibrated probabilities. "78%" reads as "a 78% chance of a tornado",
//! which no score here has been calibrated to mean, so every score a person sees is written out of
//! 100 instead, until a calibrated probability exists (Phase 8).

/// A 0..1 score as "78/100".
pub fn out_of_100(score: f32) -> String {
    format!("{:.0}/100", score.clamp(0.0, 1.0) * 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scores_read_out_of_100_never_as_percentages() {
        assert_eq!(out_of_100(0.78), "78/100");
        assert_eq!(out_of_100(0.0), "0/100");
        assert_eq!(out_of_100(1.4), "100/100");
        assert_eq!(out_of_100(f32::NAN.max(0.3)), "30/100");
        assert!(!out_of_100(0.5).contains('%'));
    }
}
