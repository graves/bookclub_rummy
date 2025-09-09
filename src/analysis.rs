use crate::card::Card;
use crate::card::ToU64;
use crate::game::calculate_best_meld_from_hand;
use crate::game::{AutoPlayDecision, Hand, PlayAction, PlayerType};
use crate::scoring::{CardVec, MELD_FUNCTIONS};
use rand::prelude::SliceRandom;
use rand::rng;
use rayon::prelude::*;
use smallvec::SmallVec;
use std::collections::{HashMap, VecDeque};

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Node {
    pub full_hand: Hand,
    pub possible_hands: Vec<PossibleHand>,
    pub possible_cards: Vec<Card>,
    pub discard_pile: VecDeque<Card>,
    pub meld_score: Option<u64>,
    pub baseline_score: u64,
    pub branches: Vec<Node>,
    pub depth: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct PossibleHand {
    pub hand: Hand,
    pub discard: Card,
    pub meld_score: u64,
}

#[derive(Clone, Debug)]
pub struct RoundProbabilities {
    pub round: usize,
    pub total_simulations: usize,
    pub baseline_score: u64,
    pub improvements: Vec<ImprovementOutcome>,
    pub probability_of_improvement: f64,
    pub expected_improvement: f64,
    pub risk_of_degradation: f64,
}

#[derive(Clone, Debug)]
pub struct ImprovementOutcome {
    pub final_score: u64,
    pub improvement: i64,
    pub probability: f64,
    pub path_count: usize,
}

#[derive(Clone, Debug, Default)]
pub struct DecisionAnalysis {
    pub conservative_choice: usize,
    pub aggressive_choice: usize,
    pub balanced_choice: usize,
}

#[derive(Clone, Debug)]
pub struct HandProbabilityAnalysis {
    pub current_baseline: u64,
    pub round_probabilities: Vec<RoundProbabilities>,
    pub optimal_stop_round: Option<usize>,
    pub confidence_level: f64,
    pub analysis_details: Option<DecisionAnalysis>,
}

#[derive(Clone, Debug)]
pub struct CardValueAnalysis {
    pub card: Card,
    pub keep_expected_value: f64,
    pub discard_expected_value: f64,
    pub net_value: f64,
    pub risk_impact: f64,
    pub strategic_value: f64,
}

#[derive(Clone, Debug)]
pub struct PlayDecision {
    pub should_play: bool,
    pub confidence: f64,
    pub reasoning: String,
    pub alternative_strategies: Vec<String>,
}

#[derive(Clone, Debug)]
struct CombinedAnalysis {
    optimal_round: usize,
    confidence: f64,
    details: DecisionAnalysis,
}

impl RoundProbabilities {
    pub fn print_score_distribution(&self) {
        println!("\nScore Distribution for Round {}:", self.round);
        println!("Total paths evaluated: {}", self.total_simulations);

        for outcome in &self.improvements {
            let percentage = outcome.probability * 100.0;
            let bar = "█".repeat((percentage / 2.0) as usize);
            println!(
                "  Score {:3}: {:6.2}% ({:5} paths) {}",
                outcome.final_score, percentage, outcome.path_count, bar
            );
        }
    }
}

pub fn evaluate_hand(node: &mut Node) -> Result<&mut Node, String> {
    // Pre-sort once and reuse - avoid repeated sorting
    node.full_hand.cards.sort_unstable(); // unstable is faster

    // Pre-allocate vectors with capacity
    let hand_len = node.full_hand.cards.len();
    let mut new_hand = CardVec::with_capacity(hand_len - 1);
    let mut meld_scores = SmallVec::<[u64; 12]>::with_capacity(12); // Assuming 12 melds

    // Pre-calculate samples once for all iterations
    let base_samples: Vec<_> = if node.depth < 3 {
        node.possible_cards.to_vec()
    } else {
        Vec::new()
    };

    for (discard_idx, &discard) in node.clone().full_hand.cards.iter().enumerate() {
        // Efficiently build new_hand without filter/collect
        new_hand.clear();
        new_hand.extend_from_slice(&node.full_hand.cards[..discard_idx]);
        new_hand.extend_from_slice(&node.full_hand.cards[discard_idx + 1..]);

        // Calculate meld scores efficiently
        meld_scores.clear();
        for &meld_fn in MELD_FUNCTIONS {
            if let Ok(score) = meld_fn(new_hand.clone()) {
                meld_scores.push(score);
            }
        }

        let max_meld_score = meld_scores.iter().copied().max().unwrap_or(0);

        if max_meld_score > 0 {
            // Minimize allocations by reusing Hand structure
            let possible_hand = PossibleHand {
                hand: Hand {
                    cards: new_hand.to_vec(),
                }, // Convert SmallVec to Vec only when storing
                discard,
                meld_score: max_meld_score,
            };

            node.possible_hands.push(possible_hand);
            node.discard_pile.push_back(discard);

            // Recursive branch evaluation with optimizations
            if node.depth < 3 && !base_samples.is_empty() {
                evaluate_branches_parallel(
                    node,
                    &new_hand,
                    &base_samples,
                    discard,
                    Some(max_meld_score),
                )?;
            }
        }
    }

    Ok(node)
}

pub fn evaluate_branches(
    node: &mut Node,
    base_hand: &CardVec,
    available_samples: &[Card],
    discard: Card,
    max_meld_score: Option<u64>,
) -> Result<(), String> {
    let mut rng = rng();

    let sample_count = 10.min(available_samples.len());

    // Early exit if no cards available
    if node.possible_cards.is_empty() {
        return Ok(());
    }

    // Use partial_shuffle for better performance than repeated choose
    let mut samples = available_samples.to_vec();
    let (selected, _) = samples.partial_shuffle(&mut rng, sample_count);

    for &mut drawn_card in selected {
        let mut simulated_hand = base_hand.clone();
        simulated_hand.push(drawn_card);

        // Create NEW available cards for this branch (don't modify parent's)
        let mut branch_available_cards = node.possible_cards.clone();
        branch_available_cards.retain(|&c| c != drawn_card); // Remove only from this branch

        // Create NEW discard pile with the discard from this simulation path
        let mut branch_discard_pile = node.discard_pile.clone();
        branch_discard_pile.push_back(discard); // Now this makes sense

        let current_depth = node.depth;
        let parent_baseline = node.baseline_score; // Pass down baseline

        // Calculate baseline for this new 6-card hand
        let new_hand = Hand {
            cards: simulated_hand.to_vec(),
        };
        let (branch_baseline, _hand) = calculate_best_meld_from_hand(&new_hand);

        // USE parent_baseline: Skip branches that can't improve
        if current_depth > 1 && branch_baseline <= parent_baseline {
            // This branch won't improve our position, skip expensive recursion
            continue;
        }

        // Create branch with minimal cloning
        let mut branch = Node {
            full_hand: Hand {
                cards: simulated_hand.to_vec(),
            },
            possible_hands: Vec::new(),
            possible_cards: branch_available_cards,
            discard_pile: branch_discard_pile,
            meld_score: max_meld_score,
            baseline_score: branch_baseline,
            branches: Vec::new(),
            depth: node.depth + 1,
        };

        evaluate_hand(&mut branch)?;
        node.branches.push(branch);
    }

    Ok(())
}

pub fn evaluate_hand_parallel(node: &mut Node) -> Result<&mut Node, String> {
    node.full_hand.cards.sort_unstable();
    let hand_len = node.full_hand.cards.len();
    let mut new_hand = CardVec::with_capacity(hand_len - 1);

    let base_samples: Vec<_> = if node.depth < 3 {
        node.possible_cards.to_vec()
    } else {
        Vec::new()
    };

    for (discard_idx, &discard) in node.clone().full_hand.cards.iter().enumerate() {
        new_hand.clear();
        new_hand.extend_from_slice(&node.full_hand.cards[..discard_idx]);
        new_hand.extend_from_slice(&node.full_hand.cards[discard_idx + 1..]);

        let scores: Vec<u64> = MELD_FUNCTIONS
            .par_iter()
            .filter_map(|&meld_fn| meld_fn(new_hand.clone()).ok())
            .collect();

        let max_meld_score = scores.iter().copied().max().unwrap_or(0);

        // ALWAYS add the possible hand, even if score is 0
        let possible_hand = PossibleHand {
            hand: Hand {
                cards: new_hand.to_vec(),
            },
            discard,
            meld_score: max_meld_score,
        };

        node.possible_hands.push(possible_hand);
        node.discard_pile.push_back(discard);

        // Continue exploring regardless of score
        if node.depth < 3 && !base_samples.is_empty() {
            if node.depth <= 1 {
                evaluate_branches_parallel(
                    node,
                    &new_hand,
                    &base_samples,
                    discard,
                    Some(max_meld_score),
                )?;
            } else {
                evaluate_branches(
                    node,
                    &new_hand,
                    &base_samples,
                    discard,
                    Some(max_meld_score),
                )?;
            }
        }
    }

    Ok(node)
}

pub fn evaluate_branches_parallel(
    node: &mut Node,
    base_hand: &CardVec,
    available_samples: &[Card],
    discard: Card,
    max_meld_score: Option<u64>,
) -> Result<(), String> {
    let mut rng = rng();
    let sample_count = 10.min(available_samples.len());

    if node.possible_cards.is_empty() {
        return Ok(());
    }

    let mut samples = available_samples.to_vec();
    let (selected, _) = samples.partial_shuffle(&mut rng, sample_count);

    let selected_cards: Vec<Card> = selected.to_vec();
    let base_hand_vec = base_hand.to_vec();
    let possible_cards = node.possible_cards.clone();
    let discard_pile = node.discard_pile.clone();
    let current_depth = node.depth;
    let parent_baseline = node.baseline_score; // Pass down baseline

    let branches: Vec<Node> = selected_cards
        .par_iter()
        .filter_map(|&drawn_card| {
            let mut simulated_hand: CardVec = base_hand_vec.clone().into();
            simulated_hand.push(drawn_card);

            let mut branch_available_cards = possible_cards.clone();
            branch_available_cards.retain(|&c| c != drawn_card);

            let mut branch_discard_pile = discard_pile.clone();
            branch_discard_pile.push_back(discard);

            // Calculate baseline for this new 6-card hand
            let new_hand = Hand {
                cards: simulated_hand.to_vec(),
            };
            let (branch_baseline, _hand) = calculate_best_meld_from_hand(&new_hand);

            // USE parent_baseline: Skip branches that can't improve
            if current_depth > 1 && branch_baseline <= parent_baseline {
                // This branch won't improve our position, skip expensive recursion
                return None;
            }

            let mut branch = Node {
                full_hand: new_hand,
                possible_hands: Vec::new(),
                possible_cards: branch_available_cards,
                discard_pile: branch_discard_pile,
                meld_score: max_meld_score,
                baseline_score: branch_baseline, // NEW: Each branch has its baseline
                branches: Vec::new(),
                depth: current_depth + 1,
            };

            match evaluate_hand(&mut branch) {
                Ok(_) => Some(branch),
                Err(_) => None,
            }
        })
        .collect();

    node.branches.extend(branches);
    Ok(())
}

impl Node {
    // Create baseline round (round 0)
    fn create_baseline_round(&self, baseline: u64) -> RoundProbabilities {
        RoundProbabilities {
            round: 0,
            total_simulations: 1,
            baseline_score: baseline,
            improvements: vec![ImprovementOutcome {
                final_score: baseline,
                improvement: 0,
                probability: 1.0,
                path_count: 1,
            }],
            probability_of_improvement: 0.0,
            expected_improvement: 0.0,
            risk_of_degradation: 0.0,
        }
    }

    /// Combine probabilities from multiple rounds
    fn combine_round_probabilities(
        &self,
        round_1: &RoundProbabilities,
        round_2: &RoundProbabilities,
        round_3: &RoundProbabilities,
        baseline: u64,
    ) -> CombinedAnalysis {
        // Calculate weighted expected values for each round
        let round_1_ev =
            round_1.expected_improvement - (round_1.risk_of_degradation * baseline as f64 * 0.5);
        let round_2_ev =
            round_2.expected_improvement - (round_2.risk_of_degradation * baseline as f64 * 0.6);
        let round_3_ev =
            round_3.expected_improvement - (round_3.risk_of_degradation * baseline as f64 * 0.7);

        // Determine optimal stopping point
        let optimal_round = if round_1_ev > round_2_ev && round_1_ev > 0.0 {
            1
        } else if round_2_ev > round_1_ev && round_2_ev > 0.0 {
            2
        } else if round_3_ev > round_2_ev && round_3_ev > 0.0 {
            3
        } else {
            0
        };

        // Calculate confidence based on probability distributions
        let confidence = if optimal_round == 0 {
            0.8 // High confidence in playing current hand
        } else {
            let round = if optimal_round == 1 {
                round_1
            } else if optimal_round == 2 {
                round_2
            } else {
                round_3
            };
            0.5 + (round.probability_of_improvement * 0.3)
                + ((1.0 - round.risk_of_degradation) * 0.2)
        };

        CombinedAnalysis {
            optimal_round,
            confidence,
            details: self.create_decision_analysis(round_1, round_2, round_3, baseline),
        }
    }

    /// Create decision analysis for different player types
    /// TODO
    fn create_decision_analysis(
        &self,
        round_1: &RoundProbabilities,
        round_2: &RoundProbabilities,
        _round_3: &RoundProbabilities,
        baseline: u64,
    ) -> DecisionAnalysis {
        let mut analysis = DecisionAnalysis::default();

        // Conservative: High risk aversion
        let conservative_1 =
            round_1.expected_improvement - (round_1.risk_of_degradation * baseline as f64 * 1.0);
        let conservative_2 =
            round_2.expected_improvement - (round_2.risk_of_degradation * baseline as f64 * 1.2);

        analysis.conservative_choice = if conservative_2 > conservative_1 && conservative_2 > 0.0 {
            2
        } else if conservative_1 > 0.0 {
            1
        } else {
            0
        };

        // Aggressive: Low risk aversion, high upside focus
        let aggressive_1 = round_1.expected_improvement
            - (round_1.risk_of_degradation * baseline as f64 * 0.2)
            + (round_1.probability_of_improvement * 5.0);
        let aggressive_2 = round_2.expected_improvement
            - (round_2.risk_of_degradation * baseline as f64 * 0.3)
            + (round_2.probability_of_improvement * 7.0);

        analysis.aggressive_choice = if aggressive_2 > aggressive_1 {
            2
        } else if aggressive_1 > 0.0 {
            1
        } else {
            0
        };

        // Balanced: Moderate risk/reward
        let balanced_1 = round_1.expected_improvement
            - (round_1.risk_of_degradation * baseline as f64 * 0.5)
            + (round_1.probability_of_improvement * 2.0);
        let balanced_2 = round_2.expected_improvement
            - (round_2.risk_of_degradation * baseline as f64 * 0.6)
            + (round_2.probability_of_improvement * 3.0);

        analysis.balanced_choice = if balanced_2 > balanced_1 && balanced_2 > 1.0 {
            2
        } else if balanced_1 > 0.5 {
            1
        } else {
            0
        };

        analysis
    }

    /// Calculate probabilities considering full 2-round tree
    pub fn calculate_cumulative_probabilities(&self) -> HandProbabilityAnalysis {
        let baseline = self.baseline_score;

        // Calculate probabilities for each round with proper path weighting
        let round_1_probs = self.analyze_round_with_paths(1, baseline);
        let round_2_probs = self.analyze_round_with_paths(2, baseline);
        let round_3_probs = self.analyze_round_with_paths(3, baseline);

        // Combine probabilities considering decision tree
        let combined_analysis = self.combine_round_probabilities(
            &round_1_probs,
            &round_2_probs,
            &round_3_probs,
            baseline,
        );

        HandProbabilityAnalysis {
            current_baseline: baseline,
            round_probabilities: vec![
                self.create_baseline_round(baseline),
                round_1_probs,
                round_2_probs,
                round_3_probs,
            ],
            optimal_stop_round: Some(combined_analysis.optimal_round),
            confidence_level: combined_analysis.confidence,
            analysis_details: Some(combined_analysis.details),
        }
    }

    /// Analyze a round considering all paths to that depth
    fn analyze_round_with_paths(&self, target_depth: usize, baseline: u64) -> RoundProbabilities {
        let mut path_outcomes: HashMap<u64, f64> = HashMap::new();
        let mut total_probability = 0.0;
        let mut total_paths = 0;

        // Collect all paths to target depth with their probabilities
        self.collect_weighted_paths(
            0,
            target_depth,
            1.0, // Starting probability
            &mut path_outcomes,
            &mut total_probability,
            &mut total_paths,
        );

        // Handle empty outcomes
        if path_outcomes.is_empty() || total_probability == 0.0 {
            return RoundProbabilities {
                round: target_depth,
                total_simulations: 0,
                baseline_score: baseline,
                improvements: vec![],
                probability_of_improvement: 0.0,
                expected_improvement: 0.0,
                risk_of_degradation: 0.0,
            };
        }

        // Normalize probabilities
        let mut improvements: Vec<ImprovementOutcome> = path_outcomes
            .into_iter()
            .map(|(score, prob)| {
                let normalized_prob = prob / total_probability;
                ImprovementOutcome {
                    final_score: score,
                    improvement: score as i64 - baseline as i64,
                    probability: normalized_prob,
                    path_count: (prob * total_paths as f64) as usize, // Approximate path count
                }
            })
            .collect();

        // Sort by score (highest first)
        improvements.sort_by(|a, b| b.final_score.cmp(&a.final_score));

        let probability_of_improvement = improvements
            .iter()
            .filter(|o| o.improvement > 0)
            .map(|o| o.probability)
            .sum();

        let expected_improvement = improvements
            .iter()
            .map(|o| o.improvement as f64 * o.probability)
            .sum();

        let risk_of_degradation = improvements
            .iter()
            .filter(|o| o.improvement < 0)
            .map(|o| o.probability)
            .sum();

        RoundProbabilities {
            round: target_depth,
            total_simulations: (total_probability * total_paths as f64) as usize,
            baseline_score: baseline,
            improvements,
            probability_of_improvement,
            expected_improvement,
            risk_of_degradation,
        }
    }

    /// Collect paths with probability weighting
    fn collect_weighted_paths(
        &self,
        current_depth: usize,
        target_depth: usize,
        current_probability: f64,
        outcomes: &mut HashMap<u64, f64>,
        total_prob: &mut f64,
        total_paths: &mut usize,
    ) {
        if current_depth == target_depth {
            // Add outcomes at target depth
            if !self.possible_hands.is_empty() {
                let mut score_distribution: HashMap<u64, usize> = HashMap::new();

                let branch_prob = current_probability / self.possible_hands.len() as f64;
                for possible_hand in &self.possible_hands {
                    *score_distribution
                        .entry(possible_hand.meld_score)
                        .or_insert(0) += 1;
                    *outcomes.entry(possible_hand.meld_score).or_insert(0.0) += branch_prob;
                    *total_prob += branch_prob;
                    *total_paths += 1;
                }
            } else {
                *outcomes.entry(self.baseline_score).or_insert(0.0) += current_probability;
                *total_prob += current_probability;
                *total_paths += 1
            }
        } else if current_depth < target_depth {
            if !self.branches.is_empty() {
                let branch_prob = current_probability / self.branches.len() as f64;
                for branch in &self.branches {
                    branch.collect_weighted_paths(
                        current_depth + 1,
                        target_depth,
                        branch_prob,
                        outcomes,
                        total_prob,
                        total_paths,
                    );
                }
            } else if !self.possible_hands.is_empty() {
                // Terminal node before target depth - use possible hands
                let branch_prob = current_probability / self.possible_hands.len() as f64;
                for possible_hand in &self.possible_hands {
                    *outcomes.entry(possible_hand.meld_score).or_insert(0.0) += branch_prob;
                    *total_prob += branch_prob;
                    *total_paths += 1;
                }
            } else {
                // No branches or possible hands - use baseline
                *outcomes.entry(self.baseline_score).or_insert(0.0) += current_probability;
                *total_prob += current_probability;
                *total_paths += 1;
            }
        }
    }

    /// Make decision considering full 3-round tree
    pub fn make_multi_round_decision(
        &self,
        player_type: PlayerType,
        prob_analysis: &HandProbabilityAnalysis,
    ) -> AutoPlayDecision {
        let baseline = prob_analysis.current_baseline as f64;

        // Calculate value of drawing 1 card vs 2 cards
        let draw_once_value = if prob_analysis.round_probabilities.len() > 1 {
            self.calculate_draw_value(&prob_analysis.round_probabilities[1], baseline)
        } else {
            0.0
        };

        let draw_twice_value = if prob_analysis.round_probabilities.len() > 2 {
            self.calculate_draw_value(&prob_analysis.round_probabilities[2], baseline)
        } else {
            0.0
        };

        let draw_thrice_value = if prob_analysis.round_probabilities.len() > 3 {
            self.calculate_draw_value(&prob_analysis.round_probabilities[3], baseline)
        } else {
            0.0
        };

        // Adjust thresholds based on player type
        let (draw_once_threshold, draw_twice_threshold, draw_thrice_threshold) = match player_type {
            PlayerType::Conservative => (1.5, 3.0, 6.0),
            PlayerType::Balanced => (0.5, 1.5, 3.0),
            PlayerType::Aggressive => (-0.5, 0.5, 1.0),
        };

        // Decide based on which option provides best value
        if draw_thrice_value > draw_twice_value && draw_thrice_value > draw_thrice_threshold {
            // Plan to draw twice (but system only allows one at a time)
            self.make_draw_decision(baseline + draw_thrice_value, 0.85)
        } else if draw_twice_value > draw_once_value && draw_twice_value > draw_twice_threshold {
            // Plan to draw twice (but system only allows one at a time)
            self.make_draw_decision(baseline + draw_twice_value, 0.8)
        } else if draw_once_value > draw_once_threshold {
            // Draw once
            self.make_draw_decision(baseline + draw_once_value, 0.75)
        } else {
            // Play current hand
            AutoPlayDecision {
                action: PlayAction::Play,
                confidence: 0.7,
                expected_score: baseline,
                card_to_discard: None,
            }
        }
    }

    fn calculate_draw_value(&self, round: &RoundProbabilities, baseline: f64) -> f64 {
        // Risk-adjusted expected value
        let risk_penalty = round.risk_of_degradation * baseline * 0.5;
        let upside_bonus = if round.probability_of_improvement > 0.5 {
            round.expected_improvement * 0.2
        } else {
            0.0
        };

        round.expected_improvement - risk_penalty + upside_bonus
    }

    fn make_draw_decision(&self, expected_score: f64, confidence: f64) -> AutoPlayDecision {
        let worst_card = self.find_worst_card_to_discard();
        AutoPlayDecision {
            action: PlayAction::Draw,
            confidence,
            expected_score,
            card_to_discard: Some(worst_card),
        }
    }

    pub fn calculate_realistic_probabilities(&self) -> HandProbabilityAnalysis {
        let baseline = self.baseline_score;

        let round_0 = RoundProbabilities {
            round: 0,
            total_simulations: 1,
            baseline_score: baseline,
            improvements: vec![ImprovementOutcome {
                final_score: baseline,
                improvement: 0,
                probability: 1.0,
                path_count: 1,
            }],
            probability_of_improvement: 0.0,
            expected_improvement: 0.0,
            risk_of_degradation: 0.0,
        };

        let mut round_probabilities = vec![round_0];

        for depth in 1..=2 {
            if let Some(round_data) = self.analyze_realistic_round(depth, baseline) {
                round_probabilities.push(round_data);
            }
        }

        let (optimal_round, analysis_details) =
            self.analyze_decision_criteria(&round_probabilities, baseline);

        HandProbabilityAnalysis {
            current_baseline: baseline,
            round_probabilities,
            optimal_stop_round: Some(optimal_round),
            confidence_level: 0.75,
            analysis_details: Some(analysis_details),
        }
    }

    fn analyze_decision_criteria(
        &self,
        rounds: &[RoundProbabilities],
        baseline: u64,
    ) -> (usize, DecisionAnalysis) {
        let mut decision_analysis = DecisionAnalysis::default();

        println!("\n=== Decision Analysis ===");

        let mut best_conservative = (0, f64::NEG_INFINITY);
        let mut best_aggressive = (0, f64::NEG_INFINITY);
        let mut best_balanced = (0, f64::NEG_INFINITY);

        for (i, round) in rounds.iter().enumerate() {
            let risk_penalty_conservative = round.risk_of_degradation * baseline as f64 * 1.0;
            let conservative_score = round.expected_improvement - risk_penalty_conservative;

            let risk_penalty_aggressive = round.risk_of_degradation * baseline as f64 * 0.2;
            let upside_bonus = if round.probability_of_improvement > 0.2 {
                round.expected_improvement * 0.3
            } else {
                0.0
            };
            let aggressive_score =
                round.expected_improvement - risk_penalty_aggressive + upside_bonus;

            let risk_penalty_balanced = round.risk_of_degradation * baseline as f64 * 0.5;
            let certainty_bonus = if round.probability_of_improvement > 0.15 {
                2.0
            } else {
                0.0
            };
            let balanced_score =
                round.expected_improvement - risk_penalty_balanced + certainty_bonus;

            println!(
                "Round {i}: Conservative={conservative_score:.2}, Aggressive={aggressive_score:.2}, Balanced={balanced_score:.2}"
            );

            if conservative_score > best_conservative.1 {
                best_conservative = (i, conservative_score);
            }
            if aggressive_score > best_aggressive.1 {
                best_aggressive = (i, aggressive_score);
            }
            if balanced_score > best_balanced.1 {
                best_balanced = (i, balanced_score);
            }
        }

        decision_analysis.conservative_choice = best_conservative.0;
        decision_analysis.aggressive_choice = best_aggressive.0;
        decision_analysis.balanced_choice = best_balanced.0;

        let optimal_round = best_conservative.0;

        println!("\nRecommendations:");
        println!(
            "  Conservative player: Stop after round {}",
            best_conservative.0
        );
        println!(
            "  Aggressive player: Stop after round {}",
            best_aggressive.0
        );
        println!("  Balanced player: Stop after round {}", best_balanced.0);
        println!("  Overall recommendation: Stop after round {optimal_round} (conservative)");

        (optimal_round, decision_analysis)
    }

    fn analyze_realistic_round(
        &self,
        target_depth: usize,
        baseline: u64,
    ) -> Option<RoundProbabilities> {
        let mut outcomes = HashMap::new();
        let mut total_simulations = 0;

        self.collect_direct_outcomes_at_depth(
            0,
            target_depth,
            &mut outcomes,
            &mut total_simulations,
        );

        if total_simulations == 0 {
            return None;
        }

        let mut improvements: Vec<ImprovementOutcome> = outcomes
            .into_iter()
            .map(|(final_score, count)| {
                let improvement = final_score as i64 - baseline as i64;
                let probability = count as f64 / total_simulations as f64;

                ImprovementOutcome {
                    final_score,
                    improvement,
                    probability,
                    path_count: count,
                }
            })
            .collect();

        improvements.sort_by(|a, b| b.final_score.cmp(&a.final_score));

        let probability_of_improvement = improvements
            .iter()
            .filter(|outcome| outcome.improvement > 0)
            .map(|outcome| outcome.probability)
            .sum();

        let expected_improvement = improvements
            .iter()
            .map(|outcome| outcome.improvement as f64 * outcome.probability)
            .sum();

        let risk_of_degradation = improvements
            .iter()
            .filter(|outcome| outcome.improvement < 0)
            .map(|outcome| outcome.probability)
            .sum();

        Some(RoundProbabilities {
            round: target_depth,
            total_simulations,
            baseline_score: baseline,
            improvements,
            probability_of_improvement,
            expected_improvement,
            risk_of_degradation,
        })
    }

    #[warn(clippy::collapsible_if)]
    fn collect_direct_outcomes_at_depth(
        &self,
        current_depth: usize,
        target_depth: usize,
        outcomes: &mut HashMap<u64, usize>,
        total_count: &mut usize,
    ) {
        if current_depth == target_depth {
            if !self.possible_hands.is_empty() {
                for possible_hand in &self.possible_hands {
                    *outcomes.entry(possible_hand.meld_score).or_insert(0) += 1;
                    *total_count += 1;
                }
            } else {
                *outcomes.entry(self.baseline_score).or_insert(0) += 1;
                *total_count += 1;
            }
        } else if current_depth < target_depth && !self.branches.is_empty() {
            for branch in &self.branches {
                branch.collect_direct_outcomes_at_depth(
                    current_depth + 1,
                    target_depth,
                    outcomes,
                    total_count,
                );
            }
        } else if current_depth < target_depth && self.branches.is_empty() {
            if !self.possible_hands.is_empty() {
                for possible_hand in &self.possible_hands {
                    *outcomes.entry(possible_hand.meld_score).or_insert(0) += 1;
                    *total_count += 1;
                }
            }
        }
    }

    /// Calculate strategic card values based on future meld potential
    pub fn calculate_strategic_card_values_correct(
        &self,
        prob_analysis: &HandProbabilityAnalysis,
    ) -> Vec<CardValueAnalysis> {
        let mut card_analyses = Vec::new();

        for &card in &self.full_hand.cards {
            let strategic_value = self.calculate_future_meld_potential(card, prob_analysis);
            card_analyses.push(strategic_value);
        }

        // Sort by strategic value (lowest first = worst to keep)
        card_analyses.sort_by(|a, b| {
            a.strategic_value
                .partial_cmp(&b.strategic_value)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        card_analyses
    }

    /// Calculate how likely this card is to contribute to future melds
    fn calculate_future_meld_potential(
        &self,
        target_card: Card,
        prob_analysis: &HandProbabilityAnalysis,
    ) -> CardValueAnalysis {
        let mut potential_scores = Vec::new();

        // Analyze this card's potential across all simulated future scenarios
        self.analyze_card_in_tree(target_card, &mut potential_scores);

        // Calculate various metrics
        let participation_rate = if potential_scores.is_empty() {
            0.0
        } else {
            potential_scores.iter().filter(|&&score| score > 0).count() as f64
                / potential_scores.len() as f64
        };

        // Immediate value (contribution to current meld)
        let immediate_value = self.calculate_immediate_contribution(target_card);

        // Future potential based on tree analysis
        let future_value = self.calculate_tree_future_potential(target_card);

        // Synergy value (how well this card works with others)
        let synergy_value = self.calculate_card_synergy(target_card);

        // Risk factor (how likely we are to need this card)
        let risk_factor = self.calculate_discard_risk(target_card, prob_analysis);

        // Strategic value combines all factors
        let strategic_value =
            immediate_value + (future_value * 0.7) + (synergy_value * 0.5) - risk_factor;

        CardValueAnalysis {
            card: target_card,
            keep_expected_value: immediate_value + future_value,
            discard_expected_value: 0.0,
            net_value: participation_rate * 10.0, // How often this card helps
            risk_impact: risk_factor,
            strategic_value,
        }
    }

    /// Analyze how this card performs across all tree scenarios
    fn analyze_card_in_tree(&self, target_card: Card, scores: &mut Vec<u64>) {
        // Check all possible hands in the tree to see how often this card contributes
        for possible_hand in &self.possible_hands {
            if possible_hand.hand.cards.contains(&target_card) {
                scores.push(possible_hand.meld_score);
            } else {
                scores.push(0); // Card wasn't kept in this scenario
            }
        }

        // Recurse into branches
        for branch in &self.branches {
            branch.analyze_card_in_tree(target_card, scores);
        }
    }

    /// Calculate immediate contribution to current hand
    fn calculate_immediate_contribution(&self, target_card: Card) -> f64 {
        let current_score = self.baseline_score as f64;
        let without_card = self.calculate_score_without_card(target_card) as f64;

        // How much does removing this card hurt the current meld?
        current_score - without_card
    }

    fn calculate_score_without_card(&self, target_card: Card) -> u64 {
        let mut remaining_cards = self.full_hand.cards.clone();
        remaining_cards.retain(|&c| c != target_card);

        if remaining_cards.len() == 5 {
            let hand_without_target = Hand {
                cards: remaining_cards,
            };
            let (score, _hand) = calculate_best_meld_from_hand(&hand_without_target);
            score
        } else {
            0
        }
    }

    /// Calculate future meld potential from tree analysis
    fn calculate_tree_future_potential(&self, target_card: Card) -> f64 {
        let mut total_potential = 0.0;
        let mut scenario_count = 0;

        // Look at all future scenarios where this card is kept
        self.collect_card_future_scenarios(target_card, &mut total_potential, &mut scenario_count);

        if scenario_count > 0 {
            total_potential / scenario_count as f64
        } else {
            0.0
        }
    }

    /// Collect future scenarios involving this card
    fn collect_card_future_scenarios(
        &self,
        target_card: Card,
        total_potential: &mut f64,
        scenario_count: &mut usize,
    ) {
        for possible_hand in &self.possible_hands {
            if possible_hand.hand.cards.contains(&target_card) {
                *total_potential += possible_hand.meld_score as f64;
                *scenario_count += 1;
            }
        }

        for branch in &self.branches {
            branch.collect_card_future_scenarios(target_card, total_potential, scenario_count);
        }
    }

    /// Calculate synergy with other cards in hand
    fn calculate_card_synergy(&self, target_card: Card) -> f64 {
        let mut synergy = 0.0;

        for &other_card in &self.full_hand.cards {
            if other_card != target_card {
                synergy += self.calculate_card_pair_synergy(target_card, other_card);
            }
        }

        synergy
    }

    /// Calculate how well two cards work together
    fn calculate_card_pair_synergy(&self, card1: Card, card2: Card) -> f64 {
        let mut synergy = 0.0;

        // Same rank (pair potential)
        if card1.rank == card2.rank {
            synergy += 5.0; // High value for pairs
        }

        // Sequential ranks (straight potential)
        let rank1 = card1.rank.to_u64().unwrap_or(0);
        let rank2 = card2.rank.to_u64().unwrap_or(0);
        let rank_diff = (rank1 as i64 - rank2 as i64).abs();

        if rank_diff <= 2 {
            synergy += 3.0 - rank_diff as f64; // Closer ranks = more synergy
        }

        // Same suit (flush potential)
        if card1.suite == card2.suite {
            synergy += 2.0;
        }

        synergy
    }

    /// Calculate risk of discarding this card
    fn calculate_discard_risk(
        &self,
        target_card: Card,
        prob_analysis: &HandProbabilityAnalysis,
    ) -> f64 {
        let mut risk = 0.0;

        // High risk if this card is essential to current meld
        let immediate_contribution = self.calculate_immediate_contribution(target_card);
        if immediate_contribution > 0.0 {
            risk += immediate_contribution * 2.0; // Double penalty for breaking current melds
        }

        // Risk based on how often this card appears in successful future scenarios
        let participation_rate = self.calculate_participation_rate(target_card);
        risk += participation_rate * 3.0;

        // Risk based on probability analysis
        if prob_analysis.round_probabilities.len() > 1 {
            let future_risk = prob_analysis.round_probabilities[1].risk_of_degradation;
            risk += future_risk * self.baseline_score as f64 * 0.1;
        }

        risk
    }

    /// Calculate how often this card participates in successful scenarios
    fn calculate_participation_rate(&self, target_card: Card) -> f64 {
        let mut participations = 0;
        let mut total_scenarios = 0;

        self.count_card_participations(target_card, &mut participations, &mut total_scenarios);

        if total_scenarios > 0 {
            participations as f64 / total_scenarios as f64
        } else {
            0.0
        }
    }

    /// Count how often this card participates in scenarios
    fn count_card_participations(
        &self,
        target_card: Card,
        participations: &mut usize,
        total_scenarios: &mut usize,
    ) {
        for possible_hand in &self.possible_hands {
            *total_scenarios += 1;
            if possible_hand.hand.cards.contains(&target_card) && possible_hand.meld_score > 0 {
                *participations += 1;
            }
        }

        for branch in &self.branches {
            branch.count_card_participations(target_card, participations, total_scenarios);
        }
    }

    pub fn make_play_decision(&self, prob_analysis: &HandProbabilityAnalysis) -> PlayDecision {
        let baseline = prob_analysis.current_baseline;
        let mut reasoning = Vec::new();
        let mut alternative_strategies = Vec::new();

        let hand_strength_threshold = 20;
        if baseline >= hand_strength_threshold {
            reasoning.push(format!("Strong current hand (score {baseline})"));
        }

        let should_continue = if prob_analysis.round_probabilities.len() > 1 {
            let round_1 = &prob_analysis.round_probabilities[1];
            let expected_final = baseline as f64 + round_1.expected_improvement;
            let success_rate = round_1.probability_of_improvement;
            let risk_rate = round_1.risk_of_degradation;

            if expected_final > baseline as f64 * 1.1 && success_rate > 0.3 && risk_rate < 0.4 {
                reasoning.push("Favorable risk/reward for drawing".to_string());
                alternative_strategies.push("Consider drawing one card".to_string());
                true
            } else {
                reasoning.push(format!(
                    "Unfavorable odds: {:.1}% success, {:.1}% risk",
                    success_rate * 100.0,
                    risk_rate * 100.0
                ));
                false
            }
        } else {
            false
        };

        let confidence = prob_analysis.confidence_level;

        let should_play = if baseline >= 30 {
            reasoning.push("Hand is strong enough to play".to_string());
            true
        } else if baseline >= 15 && !should_continue {
            reasoning.push("Medium hand, poor draw prospects".to_string());
            true
        } else if baseline < 10 && should_continue {
            reasoning.push("Weak hand, worth drawing to improve".to_string());
            alternative_strategies.push("Draw cards before deciding".to_string());
            false
        } else {
            let play = baseline >= 10;
            reasoning.push(if play {
                "Medium hand, play conservatively".to_string()
            } else {
                "Hand too weak to play".to_string()
            });
            play
        };

        PlayDecision {
            should_play,
            confidence,
            reasoning: reasoning.join("; "),
            alternative_strategies,
        }
    }

    /// Make a concrete autoplay decision for a specific player type
    pub fn make_autoplay_decision(
        &self,
        player_type: PlayerType,
        prob_analysis: &HandProbabilityAnalysis,
    ) -> AutoPlayDecision {
        let baseline = prob_analysis.current_baseline as f64;

        // Get expected score after one draw (round 1)
        let draw_expected_score = if prob_analysis.round_probabilities.len() > 1 {
            baseline + prob_analysis.round_probabilities[1].expected_improvement
        } else {
            baseline
        };

        match player_type {
            PlayerType::Conservative => {
                self.conservative_decision(baseline, draw_expected_score, prob_analysis)
            }
            PlayerType::Aggressive => {
                self.aggressive_decision(baseline, draw_expected_score, prob_analysis)
            }
            PlayerType::Balanced => {
                self.balanced_decision(baseline, draw_expected_score, prob_analysis)
            }
        }
    }

    #[warn(clippy::redundant_guards)]
    fn conservative_decision(
        &self,
        baseline: f64,
        _draw_expected_score: f64,
        prob_analysis: &HandProbabilityAnalysis,
    ) -> AutoPlayDecision {
        // Analyze both rounds to make optimal decision
        let round_1_analysis = if prob_analysis.round_probabilities.len() > 1 {
            let r1 = &prob_analysis.round_probabilities[1];
            Some((
                r1.expected_improvement - (r1.risk_of_degradation * baseline * 1.0), // Conservative risk penalty
                r1.probability_of_improvement,
                r1.expected_improvement,
            ))
        } else {
            None
        };

        let round_2_analysis = if prob_analysis.round_probabilities.len() > 2 {
            let r2 = &prob_analysis.round_probabilities[2];
            Some((
                r2.expected_improvement - (r2.risk_of_degradation * baseline * 1.2), // Higher risk penalty for 2 draws
                r2.probability_of_improvement,
                r2.expected_improvement,
            ))
        } else {
            None
        };

        let round_3_analysis = if prob_analysis.round_probabilities.len() > 3 {
            let r3 = &prob_analysis.round_probabilities[3];
            let risk_penalty = r3.risk_of_degradation * baseline * 0.6; // Slightly higher for 3 draws
            Some((
                r3.expected_improvement - risk_penalty,
                r3.probability_of_improvement,
                r3.expected_improvement,
            ))
        } else {
            None
        };

        // Weight both options (60% weight on round 1, 40% on round 2, 30% on round 3 for balanced approach)
        let (net_expected_value, best_prob, best_improvement, best_rounds) =
            match (round_1_analysis, round_2_analysis, round_3_analysis) {
                (
                    Some((r1_val, r1_prob, r1_imp)),
                    Some((r2_val, r2_prob, r2_imp)),
                    Some((r3_val, r3_prob, r3_imp)),
                ) => {
                    let weighted_value = (r1_val * 0.6) + (r2_val * 0.4) + (r3_val * 0.3);
                    if r3_val > (r1_val * 1.2 + r2_val * 1.2) {
                        // Prefer round 3 if significantly better
                        (r3_val, r3_prob, r3_imp, 2)
                    } else if r2_val > r1_val * 1.2 {
                        // Prefer round 2 if significantly better
                        (r2_val, r2_prob, r2_imp, 2)
                    } else {
                        (weighted_value, r1_prob, r1_imp, 1)
                    }
                }
                (Some((r1_val, r1_prob, r1_imp)), None, None) => (r1_val, r1_prob, r1_imp, 1),
                _ => (0.0, 0.0, 0.0, 0),
            };

        // Conservative thresholds based on baseline and best available option
        let should_draw = match baseline {
            b if b == 0.0 => true, // No meld: always draw
            b if b < 5.0 => {
                // Very weak: draw unless terrible odds
                net_expected_value > -0.5 || best_prob > 0.25
            }
            b if b < 10.0 => {
                // Weak: draw with any positive expectation
                net_expected_value > 0.5 || (best_prob > 0.35)
            }
            b if b < 15.0 => {
                // Medium-weak: draw with modest positive value
                net_expected_value > 1.0 || (best_prob > 0.45)
            }
            b if b < 20.0 => {
                // Medium: draw with good value
                net_expected_value > 2.0 || (best_prob > 0.5 && best_improvement > 3.0)
            }
            _ => {
                // Strong: draw with excellent value
                net_expected_value > 3.0 || (best_prob > 0.6 && best_improvement > baseline * 0.2)
            }
        };

        if should_draw && best_rounds > 0 {
            let worst_card = self.find_worst_card_to_discard();
            let expected_score = baseline + best_improvement;

            return AutoPlayDecision {
                action: PlayAction::Draw,
                confidence: 0.6 + (best_prob * 0.3), // Scale confidence with probability
                expected_score,
                card_to_discard: Some(worst_card),
            };
        }

        // No probability data but very weak hand - still consider drawing
        if prob_analysis.round_probabilities.is_empty() && baseline < 5.0 {
            let worst_card = self.find_worst_card_to_discard();
            return AutoPlayDecision {
                action: PlayAction::Draw,
                confidence: 0.5,
                expected_score: baseline + 2.0,
                card_to_discard: Some(worst_card),
            };
        }

        AutoPlayDecision {
            action: PlayAction::Play,
            confidence: 0.8,
            expected_score: baseline,
            card_to_discard: None,
        }
    }

    fn balanced_decision(
        &self,
        baseline: f64,
        _draw_expected_score: f64,
        prob_analysis: &HandProbabilityAnalysis,
    ) -> AutoPlayDecision {
        // Analyze both rounds with balanced risk assessment
        let round_1_analysis = if prob_analysis.round_probabilities.len() > 1 {
            let r1 = &prob_analysis.round_probabilities[1];
            let risk_penalty = r1.risk_of_degradation * baseline * 0.4; // Moderate risk penalty
            Some((
                r1.expected_improvement - risk_penalty,
                r1.probability_of_improvement,
                r1.expected_improvement,
            ))
        } else {
            None
        };

        let round_2_analysis = if prob_analysis.round_probabilities.len() > 2 {
            let r2 = &prob_analysis.round_probabilities[2];
            let risk_penalty = r2.risk_of_degradation * baseline * 0.5; // Slightly higher for 2 draws
            Some((
                r2.expected_improvement - risk_penalty,
                r2.probability_of_improvement,
                r2.expected_improvement,
            ))
        } else {
            None
        };

        let round_3_analysis = if prob_analysis.round_probabilities.len() > 3 {
            let r3 = &prob_analysis.round_probabilities[3];
            let risk_penalty = r3.risk_of_degradation * baseline * 0.6; // Slightly higher for 3 draws
            Some((
                r3.expected_improvement - risk_penalty,
                r3.probability_of_improvement,
                r3.expected_improvement,
            ))
        } else {
            None
        };

        // Weight both options (60% weight on round 1, 40% on round 2, 30% on round 3 for balanced approach)
        let (net_expected_value, best_prob, best_improvement, best_rounds) =
            match (round_1_analysis, round_2_analysis, round_3_analysis) {
                (
                    Some((r1_val, r1_prob, r1_imp)),
                    Some((r2_val, r2_prob, r2_imp)),
                    Some((r3_val, r3_prob, r3_imp)),
                ) => {
                    let weighted_value = (r1_val * 0.6) + (r2_val * 0.4) + (r3_val * 0.3);
                    if r3_val > (r1_val * 1.2 + r2_val * 1.2) {
                        // Prefer round 3 if significantly better
                        (r3_val, r3_prob, r3_imp, 2)
                    } else if r2_val > r1_val * 1.2 {
                        // Prefer round 2 if significantly better
                        (r2_val, r2_prob, r2_imp, 2)
                    } else {
                        (weighted_value, r1_prob, r1_imp, 1)
                    }
                }
                (Some((r1_val, r1_prob, r1_imp)), None, None) => (r1_val, r1_prob, r1_imp, 1),
                _ => (0.0, 0.0, 0.0, 0),
            };

        // Balanced thresholds considering three rounds
        let should_draw = match baseline {
            b if b == 0.0 => true, // No meld: always draw
            b if b < 5.0 => {
                // Very weak: draw unless terrible odds
                net_expected_value > -1.0 || best_prob > 0.05
            }
            b if b < 10.0 => {
                // Weak: draw with any positive expectation
                net_expected_value > 0.0 || (best_prob > 0.10)
            }
            b if b < 15.0 => {
                // Medium-weak: draw with modest positive value
                net_expected_value > 0.5 || (best_prob > 0.20)
            }
            b if b < 20.0 => {
                // Medium: draw with good value
                net_expected_value > 1.0 || (best_prob > 0.45 && best_improvement > 2.5)
            }
            _ => {
                // Strong: draw with excellent value
                net_expected_value > 2.0 || (best_prob > 0.5 && best_improvement > baseline * 0.15)
            }
        };

        if should_draw && best_rounds > 0 {
            let worst_card = self.find_worst_card_to_discard();
            let expected_score = baseline + best_improvement;

            return AutoPlayDecision {
                action: PlayAction::Draw,
                confidence: 0.65 + (best_prob * 0.25), // Moderate confidence scaling
                expected_score,
                card_to_discard: Some(worst_card),
            };
        }

        // No probability data but weak hand
        if prob_analysis.round_probabilities.is_empty() && baseline < 8.0 {
            let worst_card = self.find_worst_card_to_discard();
            return AutoPlayDecision {
                action: PlayAction::Draw,
                confidence: 0.6,
                expected_score: baseline + 3.0,
                card_to_discard: Some(worst_card),
            };
        }

        AutoPlayDecision {
            action: PlayAction::Play,
            confidence: 0.7,
            expected_score: baseline,
            card_to_discard: None,
        }
    }

    fn aggressive_decision(
        &self,
        baseline: f64,
        _draw_expected_score: f64,
        prob_analysis: &HandProbabilityAnalysis,
    ) -> AutoPlayDecision {
        // Analyze both rounds with minimal risk aversion
        let round_1_analysis = if prob_analysis.round_probabilities.len() > 1 {
            let r1 = &prob_analysis.round_probabilities[1];
            let risk_adjusted = r1.expected_improvement - (r1.risk_of_degradation * baseline * 0.2);
            let max_potential = r1
                .improvements
                .first()
                .map(|o| o.final_score as f64)
                .unwrap_or(baseline);
            Some((
                risk_adjusted,
                r1.probability_of_improvement,
                r1.expected_improvement,
                max_potential,
            ))
        } else {
            None
        };

        let round_2_analysis = if prob_analysis.round_probabilities.len() > 2 {
            let r2 = &prob_analysis.round_probabilities[2];
            let risk_adjusted =
                r2.expected_improvement - (r2.risk_of_degradation * baseline * 0.25);
            let max_potential = r2
                .improvements
                .first()
                .map(|o| o.final_score as f64)
                .unwrap_or(baseline);
            Some((
                risk_adjusted,
                r2.probability_of_improvement,
                r2.expected_improvement,
                max_potential,
            ))
        } else {
            None
        };
        let round_3_analysis = if prob_analysis.round_probabilities.len() > 3 {
            let r3 = &prob_analysis.round_probabilities[3];
            let risk_penalty = r3.risk_of_degradation * baseline * 0.6; // Slightly higher for 3 draws
            let max_potential = r3
                .improvements
                .first()
                .map(|o| o.final_score as f64)
                .unwrap_or(baseline);
            Some((
                r3.expected_improvement - risk_penalty,
                r3.probability_of_improvement,
                r3.expected_improvement,
                max_potential,
            ))
        } else {
            None
        };

        // Weight both options (60% weight on round 1, 40% on round 2, 30% on round 3 for balanced approach)
        let (net_expected_value, best_prob, best_improvement, max_potential, best_rounds) =
            match (round_1_analysis, round_2_analysis, round_3_analysis) {
                (
                    Some((r1_val, r1_prob, r1_imp, r1_max)),
                    Some((r2_val, r2_prob, r2_imp, r2_max)),
                    Some((r3_val, r3_prob, r3_imp, r3_max)),
                ) => {
                    let weighted_value = (r1_val * 0.6) + (r2_val * 0.4) + (r3_val * 0.3);
                    if r3_max > (((r2_max * 1.2) + (r1_max * 1.2)) / 2.0) {
                        // Prefer round 3 if significantly better
                        (r3_val, r3_prob, r3_imp, r3_max, 2)
                    } else if r2_max > r1_max * 1.2 || r2_val > r1_val {
                        // Prefer round 2 if significantly better
                        (r2_val, r2_prob, r2_imp, r2_max, 2)
                    } else {
                        (weighted_value, r1_prob, r1_imp, r1_max, 1)
                    }
                }
                (Some((r1_val, r1_prob, r1_imp, r1_max)), None, None) => {
                    (r1_val, r1_prob, r1_imp, r1_max, 1)
                }
                _ => (0.0, 0.0, 0.0, 0.0, 0),
            };

        // Calculate upside multiplier based on max potential
        let upside_multiplier = if max_potential > baseline * 2.0 {
            1.5
        } else {
            1.0
        };

        // Aggressive: very low bar for drawing
        let should_draw = best_prob > 0.2 || // Low probability threshold
            net_expected_value * upside_multiplier > -0.5 || // Accept small expected losses
            max_potential > baseline * 1.5 || // Good upside potential
            (baseline < 10.0 && best_improvement > 0.5); // Weak hand with any improvement

        if should_draw && best_rounds > 0 {
            let worst_card = self.find_worst_card_to_discard();
            // Aggressive players are optimistic about outcomes
            let expected_score = baseline + (best_improvement * 1.2).max(max_potential * 0.3);

            return AutoPlayDecision {
                action: PlayAction::Draw,
                confidence: 0.7 + (best_prob * 0.2), // High base confidence
                expected_score,
                card_to_discard: Some(worst_card),
            };
        }

        // No probability data: aggressive players still draw unless hand is strong
        if prob_analysis.round_probabilities.is_empty() && baseline < 20.0 {
            let estimated_potential = self.estimate_hand_potential();
            if estimated_potential > baseline * 0.3 {
                let worst_card = self.find_worst_card_to_discard();
                return AutoPlayDecision {
                    action: PlayAction::Draw,
                    confidence: 0.6,
                    expected_score: baseline + estimated_potential * 1.5, // Optimistic estimate
                    card_to_discard: Some(worst_card),
                };
            }
        }

        AutoPlayDecision {
            action: PlayAction::Play,
            confidence: 0.65, // Lower confidence when forced to play
            expected_score: baseline,
            card_to_discard: None,
        }
    }

    /// Estimate hand improvement potential based on hand characteristics
    fn estimate_hand_potential(&self) -> f64 {
        let cards = &self.full_hand.cards;
        let mut potential = 0.0;

        // Count pairs, near-straights, near-flushes, etc.
        let mut rank_counts = std::collections::HashMap::new();
        let mut suit_counts = std::collections::HashMap::new();

        for card in cards {
            *rank_counts.entry(card.rank).or_insert(0) += 1;
            *suit_counts.entry(card.suite).or_insert(0) += 1;
        }

        // Potential from pairs (cards that could form pairs/trips)
        for count in rank_counts.values() {
            match count {
                1 => potential += 1.0, // Could form a pair
                2 => potential += 3.0, // Could form three of a kind
                _ => {}
            }
        }

        // Potential from flushes
        for count in suit_counts.values() {
            if *count >= 3 {
                potential += (*count as f64) * 1.5; // More cards of same suit = more flush potential
            }
        }

        // Potential from straights (simplified - check for gaps)
        let mut ranks: Vec<u64> = cards.iter().map(|c| c.rank.to_u64().unwrap_or(0)).collect();
        ranks.sort();
        ranks.dedup();

        let mut consecutive_count = 1;
        let mut max_consecutive = 1;
        for i in 1..ranks.len() {
            if ranks[i] == ranks[i - 1] + 1 {
                consecutive_count += 1;
                max_consecutive = max_consecutive.max(consecutive_count);
            } else {
                consecutive_count = 1;
            }
        }

        if max_consecutive >= 3 {
            potential += max_consecutive as f64 * 2.0;
        }

        potential
    }

    /// Find the worst card to discard based on strategic analysis
    pub fn find_worst_card_to_discard(&self) -> Card {
        let dummy_prob_analysis = HandProbabilityAnalysis {
            current_baseline: self.baseline_score,
            round_probabilities: vec![],
            optimal_stop_round: Some(0),
            confidence_level: 0.5,
            analysis_details: None,
        };

        let card_values = self.calculate_strategic_card_values_correct(&dummy_prob_analysis);

        // Return the card with the lowest strategic value (worst to keep)
        card_values
            .first()
            .map(|analysis| analysis.card)
            .unwrap_or(self.full_hand.cards[0]) // Fallback to first card
    }

    /// Execute an autoplay action
    pub fn execute_autoplay_action(
        &mut self,
        action: &PlayAction,
        deck: &mut crate::game::Deck,
    ) -> Result<u64, String> {
        match action {
            PlayAction::Play => Ok(self.baseline_score),
            PlayAction::Draw => {
                // Draw one card
                if let Some(new_card) = deck.draw_pile.pop_back() {
                    self.full_hand.cards.push(new_card);

                    // Find worst card to discard from the now 6-card hand
                    let worst_card = self.find_worst_card_to_discard();

                    // Discard it
                    self.full_hand.cards.retain(|&card| card != worst_card);
                    deck.discard_pile.push_back(worst_card);

                    // Calculate final score with the new 5-card hand
                    let (score, _hand) =
                        crate::game::calculate_best_meld_from_hand(&self.full_hand);
                    self.baseline_score = score;
                    Ok(self.baseline_score)
                } else {
                    Err("No cards left in deck".to_string())
                }
            }
            PlayAction::Retrieve => {
                // Draw one card
                if let Some(new_card) = deck.discard_pile.pop_back() {
                    self.full_hand.cards.push(new_card);

                    // Find worst card to discard from the now 6-card hand
                    let worst_card = self.find_worst_card_to_discard();

                    // Discard it
                    self.full_hand.cards.retain(|&card| card != worst_card);
                    deck.discard_pile.push_back(worst_card);

                    // Calculate final score with the new 5-card hand
                    let (score, _hand) =
                        crate::game::calculate_best_meld_from_hand(&self.full_hand);
                    self.baseline_score = score;
                    Ok(self.baseline_score)
                } else {
                    Err("No cards left in deck".to_string())
                }
            }
        }
    }

    /// Debug function that prints comprehensive analysis of the current hand
    pub fn debug_advanced_round_statistics(&self) {
        let prob_analysis = self.calculate_realistic_probabilities();
        println!("{prob_analysis}");

        // Strategic analysis based on future meld potential
        println!("\n=== Strategic Card Analysis (Future-Based) ===");
        let card_values = self.calculate_strategic_card_values_correct(&prob_analysis);

        println!("Cards ranked by strategic value (lowest = best to discard):");
        for (i, card_analysis) in card_values.iter().enumerate() {
            let recommendation = if i == 0 { " ← DISCARD" } else { "" };
            println!(
                "  {}: Strategic={:.2} (Future={:.1}, Participation={:.1}%, Risk={:.1}){}",
                card_analysis.card,
                card_analysis.strategic_value,
                card_analysis.keep_expected_value,
                card_analysis.net_value,
                card_analysis.risk_impact,
                recommendation
            );
        }

        println!("\n=== Play Decision ===");
        let play_decision = self.make_play_decision(&prob_analysis);
        println!(
            "Recommendation: {}",
            if play_decision.should_play {
                "PLAY HAND"
            } else {
                "DRAW/CONTINUE"
            }
        );
        println!("Confidence: {:.1}%", play_decision.confidence * 100.0);
        println!("Reasoning: {}", play_decision.reasoning);

        if !play_decision.alternative_strategies.is_empty() {
            println!("Alternative strategies:");
            for strategy in &play_decision.alternative_strategies {
                println!("  - {strategy}");
            }
        }

        // Autoplay decisions for different player types
        let player_types = [
            PlayerType::Conservative,
            PlayerType::Aggressive,
            PlayerType::Balanced,
        ];

        println!("\n=== Autoplay Decisions ===");
        for player_type in &player_types {
            let decision = self.make_autoplay_decision(player_type.clone(), &prob_analysis);

            println!("\n{player_type:?} Player:");
            println!("  Decision: {:?}", decision.action);
            println!("  Confidence: {:.1}%", decision.confidence * 100.0);
            println!("  Expected Score: {:.1}", decision.expected_score);

            if let Some(card) = decision.card_to_discard {
                println!("  Card to discard: {card}");
            }

            match decision.action {
                PlayAction::Play => println!("  → Will play current hand"),
                PlayAction::Draw => println!("  → Will draw one card and discard worst card"),
                PlayAction::Retrieve => {
                    println!("  → Will retrieve the discard and discard worst card")
                }
            }
        }
    }
}
