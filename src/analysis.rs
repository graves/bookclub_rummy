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

pub fn evaluate_hand(node: &mut Node) -> Result<&mut Node, String> {
    // Pre-sort once and reuse - avoid repeated sorting
    node.full_hand.cards.sort_unstable(); // unstable is faster

    // Pre-allocate vectors with capacity
    let hand_len = node.full_hand.cards.len();
    let mut new_hand = CardVec::with_capacity(hand_len - 1);
    let mut meld_scores = SmallVec::<[u64; 12]>::with_capacity(12); // Assuming 12 melds

    // Pre-calculate samples once for all iterations
    let base_samples: Vec<_> = if node.depth < 2 {
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
            if let Some(score) = meld_fn(new_hand.clone()).ok() {
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
            if node.depth < 2 && !base_samples.is_empty() {
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

    let sample_count = 27.min(available_samples.len());

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
        let branch_baseline = calculate_best_meld_from_hand(&new_hand);

        // USE parent_baseline: Skip branches that can't improve
        if current_depth > 1 && branch_baseline <= parent_baseline {
            // This branch won't improve our position, skip expensive recursion
            return Ok(());
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
            baseline_score: 0,
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
    let mut meld_scores = SmallVec::<[u64; 12]>::with_capacity(12);

    let base_samples: Vec<_> = if node.depth < 2 {
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
        if node.depth < 2 && !base_samples.is_empty() {
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
    let sample_count = 27.min(available_samples.len());

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
            let branch_baseline = calculate_best_meld_from_hand(&new_hand);

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
                "Round {}: Conservative={:.2}, Aggressive={:.2}, Balanced={:.2}",
                i, conservative_score, aggressive_score, balanced_score
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
        println!(
            "  Overall recommendation: Stop after round {} (conservative)",
            optimal_round
        );

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
            calculate_best_meld_from_hand(&hand_without_target)
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
            reasoning.push(format!("Strong current hand (score {})", baseline));
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

    fn conservative_decision(
    &self,
    baseline: f64,
    draw_expected_score: f64,
    prob_analysis: &HandProbabilityAnalysis
    ) -> AutoPlayDecision {
        if prob_analysis.round_probabilities.len() > 1 {
            let round_1 = &prob_analysis.round_probabilities[1];
        
            // Conservative logic: depends heavily on current hand strength
            let is_very_weak_hand = baseline < 5.0;
            let is_weak_hand = baseline < 10.0;
            let is_decent_hand = baseline >= 15.0;
        
            if is_very_weak_hand {
                // Very weak hands (0-4 points): Conservative players will draw if improvement is likely
                // They can't afford to play such weak hands
                if round_1.probability_of_improvement > 0.6 && 
                   round_1.expected_improvement > 1.0 &&
                   round_1.risk_of_degradation < 0.3 {
                
                    let worst_card = self.find_worst_card_to_discard();
                    return AutoPlayDecision {
                        action: PlayAction::Draw,
                        confidence: 0.7,
                        expected_score: draw_expected_score,
                        card_to_discard: Some(worst_card),
                    };
                }
            } else if is_weak_hand {
                // Weak hands (5-9 points): More selective, but still willing to improve
                let risk_adjusted_improvement = round_1.expected_improvement - 
                    (round_1.risk_of_degradation * baseline * 1.5);
            
                if round_1.probability_of_improvement > 0.75 &&
                   risk_adjusted_improvement > 2.0 &&
                   round_1.risk_of_degradation < 0.2 {
                
                    let worst_card = self.find_worst_card_to_discard();
                    return AutoPlayDecision {
                        action: PlayAction::Draw,
                        confidence: 0.6,
                        expected_score: draw_expected_score,
                        card_to_discard: Some(worst_card),
                    };
                }
            } else if is_decent_hand {
                // Decent hands (15+ points): Very conservative, only draw with excellent prospects
                let risk_adjusted_improvement = round_1.expected_improvement - 
                    (round_1.risk_of_degradation * baseline * 2.0);
            
                if round_1.probability_of_improvement > 0.8 &&
                   risk_adjusted_improvement > baseline * 0.3 &&
                   round_1.risk_of_degradation < 0.1 {
                
                    let worst_card = self.find_worst_card_to_discard();
                    return AutoPlayDecision {
                        action: PlayAction::Draw,
                        confidence: 0.5,
                        expected_score: draw_expected_score,
                        card_to_discard: Some(worst_card),
                    };
                }
            }
            // Medium hands (10-14 points): Conservative players are happy to play these
        } else {
            // No probability data: Conservative players only draw very weak hands
            if baseline < 3.0 {
                let estimated_potential = self.estimate_hand_potential();
                if estimated_potential > 2.0 {
                    let worst_card = self.find_worst_card_to_discard();
                    return AutoPlayDecision {
                        action: PlayAction::Draw,
                        confidence: 0.5,
                        expected_score: baseline + estimated_potential * 0.5, // Conservative estimate
                        card_to_discard: Some(worst_card),
                    };
                }
            }
        }

        // Conservative default: Play the current hand
        AutoPlayDecision {
            action: PlayAction::Play,
            confidence: 0.8,
            expected_score: baseline,
            card_to_discard: None,
        }
    }

    fn aggressive_decision(
        &self,
        baseline: f64,
        draw_expected_score: f64,
        prob_analysis: &HandProbabilityAnalysis
    ) -> AutoPlayDecision {
        if prob_analysis.round_probabilities.len() > 1 {
            let round_1 = &prob_analysis.round_probabilities[1];
        
            // Aggressive: Low bar for drawing, focus on upside potential
            let risk_adjusted_improvement = round_1.expected_improvement - 
                (round_1.risk_of_degradation * baseline * 0.2); // Low risk penalty
        
            // Look at the maximum possible outcome, not just expected
            let max_potential = round_1.improvements.first()
                .map(|outcome| outcome.final_score as f64)
                .unwrap_or(baseline);
        
            let upside_multiplier = if max_potential > baseline * 2.0 { 1.5 } else { 1.0 };
        
            // Aggressive: Draw if any reasonable chance of improvement
            if round_1.probability_of_improvement > 0.25 || // Low threshold for improvement chance
               risk_adjusted_improvement * upside_multiplier > 0.0 || // Any positive expected value
               max_potential > baseline * 1.8 { // High upside potential available
            
                let worst_card = self.find_worst_card_to_discard();
                return AutoPlayDecision {
                    action: PlayAction::Draw,
                    confidence: 0.8,
                    expected_score: draw_expected_score,
                    card_to_discard: Some(worst_card),
                };
            }
        } else {
            // No probability data: aggressive players assume drawing is worth it unless baseline is excellent
            let estimated_improvement_potential = self.estimate_hand_potential();
            if estimated_improvement_potential > baseline * 0.5 {
                let worst_card = self.find_worst_card_to_discard();
                return AutoPlayDecision {
                    action: PlayAction::Draw,
                    confidence: 0.6,
                    expected_score: baseline + estimated_improvement_potential,
                    card_to_discard: Some(worst_card),
                };
            }
        }

        // Aggressive: Only play if the probabilities really don't favor drawing
        AutoPlayDecision {
            action: PlayAction::Play,
            confidence: 0.7,
            expected_score: baseline,
            card_to_discard: None,
        }
    }

    fn balanced_decision(
        &self,
        baseline: f64,
        draw_expected_score: f64,
        prob_analysis: &HandProbabilityAnalysis
    ) -> AutoPlayDecision {
        if prob_analysis.round_probabilities.len() > 1 {
            let round_1 = &prob_analysis.round_probabilities[1];
        
            // Balanced: Make decision based on risk-adjusted expected value
            let risk_penalty = round_1.risk_of_degradation * baseline * 0.6; // Moderate risk aversion
            let net_expected_value = round_1.expected_improvement - risk_penalty;
        
            // Also consider the probability of improvement vs. the current baseline
            let improvement_ratio = if baseline > 0.0 {
                round_1.expected_improvement / baseline
            } else {
                round_1.expected_improvement // Any improvement is good if baseline is 0
            };
        
            // Balanced thresholds based on probabilities
            let should_draw = 
                // Good expected value after risk adjustment
                net_expected_value > 1.0 ||
                // Decent chance of improvement with meaningful gain
                (round_1.probability_of_improvement > 0.5 && improvement_ratio > 0.2) ||
                // Low baseline with reasonable improvement prospects
                (baseline < 5.0 && round_1.expected_improvement > 2.0) ||
                // High probability of any improvement when current hand is weak
                (baseline < 2.0 && round_1.probability_of_improvement > 0.6);
        
            if should_draw {
                let worst_card = self.find_worst_card_to_discard();
                return AutoPlayDecision {
                    action: PlayAction::Draw,
                    confidence: 0.75,
                    expected_score: draw_expected_score,
                    card_to_discard: Some(worst_card),
                };
            }
        } else {
            // No probability data: estimate based on hand characteristics
            let estimated_potential = self.estimate_hand_potential();
            let potential_ratio = estimated_potential / baseline.max(1.0);
        
            // Balanced players draw if estimated potential is reasonably high
            if potential_ratio > 0.4 {
                let worst_card = self.find_worst_card_to_discard();
                return AutoPlayDecision {
                    action: PlayAction::Draw,
                    confidence: 0.6,
                    expected_score: baseline + estimated_potential,
                    card_to_discard: Some(worst_card),
                };
            }
        }

        // Balanced: Play the current hand if drawing doesn't look promising
        AutoPlayDecision {
            action: PlayAction::Play,
            confidence: 0.7,
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
        let mut ranks: Vec<u64> = cards.iter()
            .map(|c| c.rank.to_u64().unwrap_or(0))
            .collect();
        ranks.sort();
        ranks.dedup();
    
        let mut consecutive_count = 1;
        let mut max_consecutive = 1;
        for i in 1..ranks.len() {
            if ranks[i] == ranks[i-1] + 1 {
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
    fn find_worst_card_to_discard(&self) -> Card {
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
                    self.baseline_score =
                        crate::game::calculate_best_meld_from_hand(&self.full_hand);
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
        println!("{}", prob_analysis);

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
                println!("  - {}", strategy);
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

            println!("\n{:?} Player:", player_type);
            println!("  Decision: {:?}", decision.action);
            println!("  Confidence: {:.1}%", decision.confidence * 100.0);
            println!("  Expected Score: {:.1}", decision.expected_score);

            if let Some(card) = decision.card_to_discard {
                println!("  Card to discard: {}", card);
            }

            match decision.action {
                PlayAction::Play => println!("  → Will play current hand"),
                PlayAction::Draw => println!("  → Will draw one card and discard worst card"),
            }
        }
    }
}
