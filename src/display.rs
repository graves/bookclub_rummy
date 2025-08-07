use crate::analysis::{HandProbabilityAnalysis, RoundProbabilities};
use crate::card::Card;
use crate::game::Hand;
use std::fmt;

impl fmt::Display for Card {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}{}",
            self.name.to_string().unwrap(),
            self.suite.to_char().unwrap()
        )
    }
}

impl fmt::Display for Hand {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut hand = "".to_string();
        for c in self.cards.clone() {
            hand = format!("{hand} {c}");
        }
        write!(f, "{hand}")
    }
}

impl fmt::Display for RoundProbabilities {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let round_description = match self.round {
            0 => "Current Hand".to_string(),
            1 => "After 1st Draw".to_string(),
            2 => "After 2nd Draw".to_string(),
            3 => "After 3rd Draw".to_string(),
            n => format!("After {}th Draw", n),
        };

        writeln!(
            f,
            "Round {} - {} (baseline: {}):",
            self.round, round_description, self.baseline_score
        )?;
        writeln!(f, "  Total simulations: {}", self.total_simulations)?;
        writeln!(
            f,
            "  Probability of improvement: {:.1}%",
            self.probability_of_improvement * 100.0
        )?;
        writeln!(
            f,
            "  Expected improvement: {:+.2}",
            self.expected_improvement
        )?;
        writeln!(
            f,
            "  Risk of degradation: {:.1}%",
            self.risk_of_degradation * 100.0
        )?;

        writeln!(f, "  Top outcomes:")?;
        for outcome in self.improvements.iter().take(5) {
            writeln!(
                f,
                "    Score {}: {:+} ({:.1}% chance, {} paths)",
                outcome.final_score,
                outcome.improvement,
                outcome.probability * 100.0,
                outcome.path_count
            )?;
        }

        Ok(())
    }
}

impl fmt::Display for HandProbabilityAnalysis {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "=== Conditional Hand Analysis ===")?;
        writeln!(f, "Current baseline score: {}", self.current_baseline)?;
        writeln!(
            f,
            "Analysis confidence: {:.1}%",
            self.confidence_level * 100.0
        )?;

        if let Some(optimal_round) = self.optimal_stop_round {
            writeln!(
                f,
                "Recommended strategy: Stop after round {}",
                optimal_round
            )?;
        }

        writeln!(f)?;
        writeln!(f, "Expected outcomes if you play to each round:")?;
        for round_prob in &self.round_probabilities {
            writeln!(
                f,
                "Round {}: Expected score = {:.1}, Risk of loss = {:.1}%",
                round_prob.round,
                self.current_baseline as f64 + round_prob.expected_improvement,
                round_prob.risk_of_degradation * 100.0
            )?;
        }

        writeln!(f)?;
        for round_prob in &self.round_probabilities {
            writeln!(f, "{}", round_prob)?;
        }

        Ok(())
    }
}
