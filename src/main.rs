use std::io;
use std::{collections::VecDeque, ops::Index};

use rand::seq::SliceRandom;
use rummy::{analysis::*, card::*, game::*};

fn main() {
    let mut shuffled_deck = shuffle_deck().unwrap();
    let mut deck = Deck {
        draw_pile: &mut shuffled_deck,
        discard_pile: &mut VecDeque::new(),
    };
    let player1 = Player {
        name: "Thomas Gentry".to_string(),
        player_type: None,
        hand: Hand { cards: Vec::new() },
        actions: VecDeque::new(),
        dialogue: VecDeque::new(),
    };
    let player2 = Player {
        name: "Socrates".to_string(),
        player_type: Some(PlayerType::Conservative),
        hand: Hand { cards: Vec::new() },
        actions: VecDeque::new(),
        dialogue: VecDeque::new(),
    };
    let player3 = Player {
        name: "W.E.B. Du Bois".to_string(),
        player_type: Some(PlayerType::Balanced),
        hand: Hand { cards: Vec::new() },
        actions: VecDeque::new(),
        dialogue: VecDeque::new(),
    };
    let player4 = Player {
        name: "Thomas Sankara".to_string(),
        player_type: Some(PlayerType::Aggressive),
        hand: Hand { cards: Vec::new() },
        actions: VecDeque::new(),
        dialogue: VecDeque::new(),
    };

    let mut rng = rand::rng();
    let mut players = vec![player1, player2, player3, player4];
    players.shuffle(&mut rng);

    // Deal cards
    let (players, deck) = deal_cards(players, &mut deck).unwrap();
    // Flip first discard card
    let visible_discard = deck.draw_pile.pop_back().unwrap();
    deck.discard_pile.push_back(visible_discard);

    for mut player in players.clone() {
        if let Some(player_type) = player.player_type {
            let mut hand = player.hand.clone();
            hand.cards.push(*deck.discard_pile.iter().last().unwrap());

            let baseline_score = calculate_best_meld_from_hand(&hand);

            let possible_cards: Vec<Card> = deck.draw_pile.iter().cloned().collect::<Vec<Card>>();
            let node = Node {
                full_hand: hand.clone(),
                possible_hands: Vec::new(),
                possible_cards: possible_cards,
                discard_pile: deck.discard_pile.clone(),
                meld_score: None,
                baseline_score: baseline_score,
                branches: Vec::new(),
                depth: 0,
            };

            let prob_analysis = node.calculate_cumulative_probabilities();
            let decision = node.make_autoplay_decision(player_type, &prob_analysis);

            let name = player.name;

            match decision.action {
                PlayAction::Play => {
                    println!("{name} → Will play current hand");
                    let h = player.hand;
                    println!("HAND: {h}");
                    return;
                }
                PlayAction::Draw => {
                    // Draw the actual card
                    let drawn_card = deck.draw_pile.pop_back();
                    if let Some(drawn_card) = drawn_card {
                        player.hand.cards.push(drawn_card);
                    } else {
                        deck.reshuffle_deck();
                        let drawn_card = deck.draw_pile.pop_back().unwrap();

                        player.hand.cards.push(drawn_card);
                    }

                    // NOW create node with the ACTUAL 6-card hand
                    let actual_hand = player.hand.clone();
                    let node = Node {
                        full_hand: actual_hand,
                        possible_hands: Vec::new(),
                        possible_cards: deck.draw_pile.iter().cloned().collect(),
                        discard_pile: deck.discard_pile.clone(),
                        meld_score: None,
                        baseline_score: calculate_best_meld_from_hand(&player.hand),
                        branches: Vec::new(),
                        depth: 0,
                    };

                    // Find worst card from ACTUAL hand
                    let mut discard_card = node.find_worst_card_to_discard();

                    let h = player.hand.clone();

                    let idx = player
                        .hand
                        .cards
                        .iter()
                        .position(|c| *c == discard_card)
                        .unwrap();

                    discard_card = player.hand.cards.remove(idx);
                    deck.discard_pile.push_back(discard_card);

                    let action = ActionHistory {
                        choice: Choice::Draw,
                        card_to_discard: decision.card_to_discard.clone(),
                    };

                    player.actions.push_back(action);
                }
            }
        } else {
            let everyone_else = players.clone();
            players.clone().retain(|p| *p == player);

            World {
                players: everyone_else,
                you: player.clone(),
            }
            .print();

            let mut player_choice: Option<Choice> = None;

            while player_choice.is_none() {
                player_choice = match parse_choice() {
                    Ok(choice) => Some(choice),
                    Err(err) => {
                        eprintln!("Error: {err}");
                        None
                    }
                }
            }

            match player_choice {
                Some(Choice::Draw) => {
                    // Draw card
                    let drawn_card = deck.draw_pile.pop_back();
                    if let Some(drawn_card) = drawn_card {
                        player.hand.cards.push(drawn_card);
                    } else {
                        deck.reshuffle_deck();
                        let drawn_card = deck.draw_pile.pop_back().unwrap();

                        player.hand.cards.push(drawn_card);
                    }

                    let mut discard_choice = None;
                    while discard_choice.is_none() {
                        let h = player.hand.clone();
                        println!("NEW HAND: {h}");

                        discard_choice = match parse_discard() {
                            Ok(discard) => Some(discard),
                            Err(err) => {
                                eprintln!("Error: {err}");
                                None
                            }
                        }
                    }

                    let mut player_choice: Option<Choice> = None;

                    while player_choice.is_none() {
                        player_choice = match parse_choice() {
                            Ok(choice) => Some(choice),
                            Err(err) => {
                                eprintln!("Error: {err}");
                                None
                            }
                        }
                    }

                    let mut can_continue = false;
                    while !can_continue {
                        match player_choice {
                            Some(Choice::Play) => {
                                let meld_score = calculate_best_meld_from_hand(&player.hand);
                                println!("You scored: {meld_score}");
                                return (());
                            }
                            Some(Choice::Fold) => {
                                let action = ActionHistory {
                                    choice: Choice::Draw,
                                    card_to_discard: discard_choice,
                                };

                                let mut discard_card = action.card_to_discard.unwrap();
                                let idx = player
                                    .hand
                                    .cards
                                    .iter()
                                    .position(|c| *c == discard_card)
                                    .unwrap();

                                discard_card = player.hand.cards.remove(idx);
                                deck.discard_pile.push_back(discard_card);

                                player.actions.push_back(action);
                                can_continue = true
                            }
                            _ => {
                                eprintln!("Must draw or fold!");
                            }
                        }
                    }
                }
                Some(Choice::Play) => {
                    let discard_visible = deck.discard_pile.pop_back().unwrap();
                    player.hand.cards.push(discard_visible);
                    let meld_score = calculate_best_meld_from_hand(&player.hand);
                    println!("You scored: {meld_score}");
                    return (());
                }
                _ => panic!("Unreachable code!"),
            }
        }
    }

    fn parse_choice() -> Result<Choice, String> {
        println!("\nWhat do you want to do? ");

        let mut input = String::new();
        io::stdin()
            .read_line(&mut input)
            .expect("Failed to read line");

        // Remove any trailing newline
        let input = input.trim();

        match input {
            "D" | "d" => Ok(Choice::Draw),
            "P" | "p" => Ok(Choice::Play),
            "F" | "f" => Ok(Choice::Fold),
            _ => Err("Invalid input. Expected D[d] or P[p].".into()),
        }
    }

    fn parse_discard() -> Result<Card, String> {
        println!("\nWhich card would you like to discard? ");

        let mut input = String::new();
        io::stdin()
            .read_line(&mut input)
            .expect("Failed to read line");

        // Remove any trailing newline
        let input = input.trim().to_string();

        Card::from_string(input)
    }
}
