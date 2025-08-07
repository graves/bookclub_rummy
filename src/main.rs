use std::collections::VecDeque;

use rummy::{analysis::*, card::*, game::*};

fn main() {
    let mut shuffled_deck = shuffle_deck().unwrap();
    let mut deck = Deck {
        draw_pile: &mut shuffled_deck,
        discard_pile: &mut VecDeque::new(),
    };
    let player1 = Player {
        name: "Player 1".to_string(),
        hand: Hand { cards: Vec::new() },
    };
    let player2 = Player {
        name: "Player 2".to_string(),
        hand: Hand { cards: Vec::new() },
    };
    let players = vec![player1, player2];

    // Deal cards
    let (mut players, deck) = deal_cards(players, &mut deck).unwrap();

    // Draw card
    let drawn_card = deck.draw_pile.pop_back().unwrap();
    players[0].hand.cards.push(drawn_card);

    let hand = players[0].hand.clone();
    let baseline_score = calculate_best_meld_from_hand(&hand);

    let possible_cards: Vec<Card> = deck.draw_pile.iter().cloned().collect::<Vec<Card>>();
    let mut node = Node {
        full_hand: hand.clone(),
        possible_hands: Vec::new(),
        possible_cards: possible_cards,
        discard_pile: deck.discard_pile.clone(),
        meld_score: None,
        baseline_score: baseline_score,
        branches: Vec::new(),
        depth: 0,
    };

    println!("Starting hand evaluation...");
    let start_time = std::time::Instant::now();

    println!("HAND: {}", hand);
    println!("BASELINE SCORE: {}", calculate_best_meld_from_hand(&hand));

    let tree = evaluate_hand_parallel(&mut node).unwrap();
    let node = &tree;

    let elapsed = start_time.elapsed();
    println!("Hand evaluation completed in {:?}", elapsed);

    // All the complex analysis is now in one clean method call
    //node.debug_advanced_round_statistics();

    let prob_analysis = node.calculate_realistic_probabilities();

    // Test all three player types
    let player_types = [
        PlayerType::Conservative,
        PlayerType::Aggressive,
        PlayerType::Balanced,
    ];

    println!("\n=== Autoplay Decisions ===");
    for player_type in &player_types {
        let decision = node.make_autoplay_decision(player_type.clone(), &prob_analysis);

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
