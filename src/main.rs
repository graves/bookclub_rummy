use std::collections::VecDeque;
use std::io::{self, Write};

use rand::seq::SliceRandom;
use rummy::{analysis::*, card::*, game::*};

struct GameState<'a> {
    players: Vec<Player>,
    deck: &'a mut Deck<'a>,
    actions_log: Vec<String>,
    messages: Vec<String>,
    current_player_idx: usize,
}

impl<'a> GameState<'a> {
    fn clear_screen() {
        print!("\x1B[2J\x1B[1;1H"); // ANSI escape codes to clear screen and move cursor to top
        io::stdout().flush().unwrap();
    }

    fn display(&self, human_player: &Player) {
        Self::clear_screen();

        // Header
        println!("Today's Bookclub Rummy is on East of Eden by John Steinbeck\n");

        // Player dialogue (random quotes or actual game dialogue)
        for player in &self.players {
            if player.player_type.is_some() {
                let dialogue = self.get_player_dialogue(&player.name);
                println!("{:20} says: {}", player.name, dialogue);
            }
        }
        println!();

        // Display discard pile and draw pile indicator
        if let Some(top_card) = self.deck.discard_pile.back() {
            println!("[{}] [⌧]", top_card);
        } else {
            println!("[--] [⌧]");
        }

        // Display human player's hand
        print!(" ");
        for card in &human_player.hand.cards {
            print!("{} ", card);
        }
        println!("\n");

        // Input prompt
        print!("What do you want to do? ");
        io::stdout().flush().unwrap();
    }

    fn display_with_actions(&self, human_player: &Player) {
        self.display(human_player);

        // Show recent actions
        if !self.actions_log.is_empty() {
            println!("\nActions:");
            let start = self.actions_log.len().saturating_sub(3);
            for action in &self.actions_log[start..] {
                println!("{}", action);
            }
        }

        // Show messages/errors
        if !self.messages.is_empty() {
            println!("\nMessages:");
            for msg in &self.messages {
                println!("{}", msg);
            }
        }
    }

    fn get_player_dialogue(&self, name: &str) -> &str {
        match name {
            "Socrates" => "I am not annoying to those who can annoy.",
            "W.E.B. Du Bois" => "I invented sociology and nobody talks about it.",
            "Thomas Sankara" => {
                "If you forget what Burkina Faso means one more time, I'm making you move to Africa to continue the liberation of our women."
            }
            _ => "...",
        }
    }

    fn add_action(&mut self, player_name: &str, action: &str, card: Option<Card>) {
        let action_text = if let Some(card) = card {
            format!("{:20} {} {}", player_name, action, card)
        } else {
            format!("{:20} {}", player_name, action)
        };
        self.actions_log.push(action_text);
    }

    fn add_message(&mut self, msg: String) {
        self.messages.push(msg);
        // Keep only last 3 messages
        if self.messages.len() > 3 {
            self.messages.remove(0);
        }
    }

    fn clear_messages(&mut self) {
        self.messages.clear();
    }
}

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
    let (mut players, mut deck) = deal_cards(players, &mut deck).unwrap();

    // Flip first discard card
    let visible_discard = deck.draw_pile.pop_back().unwrap();
    deck.discard_pile.push_back(visible_discard);

    let mut game_state = GameState {
        players: players.clone(),
        deck,
        actions_log: Vec::new(),
        messages: Vec::new(),
        current_player_idx: 0,
    };

    // Main game loop
    loop {
        let current_player = &mut players[game_state.current_player_idx];

        if let Some(player_type) = current_player.player_type {
            // AI player turn
            let mut hand = current_player.hand.clone();
            hand.cards
                .push(*game_state.deck.discard_pile.back().unwrap());

            let baseline_score = calculate_best_meld_from_hand(&hand);
            let possible_cards: Vec<Card> = game_state.deck.draw_pile.iter().cloned().collect();

            let node = Node {
                full_hand: hand.clone(),
                possible_hands: Vec::new(),
                possible_cards,
                discard_pile: game_state.deck.discard_pile.clone(),
                meld_score: None,
                baseline_score,
                branches: Vec::new(),
                depth: 0,
            };

            let prob_analysis = node.calculate_cumulative_probabilities();
            let decision = node.make_autoplay_decision(player_type, &prob_analysis);

            match decision.action {
                PlayAction::Play => {
                    let score = calculate_best_meld_from_hand(&current_player.hand);
                    game_state.add_action(&current_player.name, "played their hand!", None);
                    println!(
                        "\n{} played their hand with score: {}",
                        current_player.name, score
                    );
                    return;
                }
                PlayAction::Draw => {
                    // Draw card
                    let drawn_card = if let Some(card) = game_state.deck.draw_pile.pop_back() {
                        card
                    } else {
                        game_state.deck.reshuffle_deck();
                        game_state.deck.draw_pile.pop_back().unwrap()
                    };

                    current_player.hand.cards.push(drawn_card);

                    // Find worst card to discard
                    let actual_hand = current_player.hand.clone();
                    let node = Node {
                        full_hand: actual_hand,
                        possible_hands: Vec::new(),
                        possible_cards: game_state.deck.draw_pile.iter().cloned().collect(),
                        discard_pile: game_state.deck.discard_pile.clone(),
                        meld_score: None,
                        baseline_score: calculate_best_meld_from_hand(&current_player.hand),
                        branches: Vec::new(),
                        depth: 0,
                    };

                    let discard_card = node.find_worst_card_to_discard();
                    let idx = current_player
                        .hand
                        .cards
                        .iter()
                        .position(|c| *c == discard_card)
                        .unwrap();
                    let discarded = current_player.hand.cards.remove(idx);
                    game_state.deck.discard_pile.push_back(discarded);

                    game_state.add_action(
                        &current_player.name,
                        "drew and discarded the",
                        Some(discarded),
                    );
                }
            }
        } else {
            // Human player turn
            game_state.clear_messages();
            game_state.display_with_actions(current_player);

            let mut player_choice = None;
            while player_choice.is_none() {
                let mut input = String::new();
                io::stdin()
                    .read_line(&mut input)
                    .expect("Failed to read line");

                match parse_choice(input.trim()) {
                    Ok(choice) => player_choice = Some(choice),
                    Err(err) => {
                        game_state.add_message(err);
                        game_state.display_with_actions(current_player);
                    }
                }
            }

            match player_choice.unwrap() {
                Choice::Draw => {
                    // Draw card
                    let drawn_card = if let Some(card) = game_state.deck.draw_pile.pop_back() {
                        card
                    } else {
                        game_state.deck.reshuffle_deck();
                        game_state.deck.draw_pile.pop_back().unwrap()
                    };

                    current_player.hand.cards.push(drawn_card);

                    // Show new hand and ask for discard
                    game_state.clear_messages();
                    game_state.display_with_actions(current_player);
                    println!("\nWhich card would you like to discard?");

                    let mut discard_card = None;
                    while discard_card.is_none() {
                        let mut input = String::new();
                        io::stdin()
                            .read_line(&mut input)
                            .expect("Failed to read line");

                        match Card::from_string(input.trim().to_string()) {
                            Ok(card) => {
                                if current_player.hand.cards.contains(&card) {
                                    discard_card = Some(card);
                                } else {
                                    game_state.add_message("You don't have that card!".to_string());
                                    game_state.display_with_actions(current_player);
                                }
                            }
                            Err(err) => {
                                game_state.add_message(err);
                                game_state.display_with_actions(current_player);
                            }
                        }
                    }

                    let card = discard_card.unwrap();
                    let idx = current_player
                        .hand
                        .cards
                        .iter()
                        .position(|c| *c == card)
                        .unwrap();
                    current_player.hand.cards.remove(idx);
                    game_state.deck.discard_pile.push_back(card);

                    game_state.add_action(
                        &current_player.name,
                        "drew and discarded the",
                        Some(card),
                    );

                    // Ask if they want to play or fold
                    game_state.display_with_actions(current_player);
                    println!("\nPlay (P) or Fold (F)?");

                    loop {
                        let mut input = String::new();
                        io::stdin()
                            .read_line(&mut input)
                            .expect("Failed to read line");

                        match input.trim().to_lowercase().as_str() {
                            "p" | "play" => {
                                let score = calculate_best_meld_from_hand(&current_player.hand);
                                println!("\nYou played your hand with score: {}", score);
                                return;
                            }
                            "f" | "fold" => break,
                            _ => {
                                game_state
                                    .add_message("Please enter P (play) or F (fold)".to_string());
                                game_state.display_with_actions(current_player);
                            }
                        }
                    }
                }
                Choice::Play => {
                    let discard_visible = game_state.deck.discard_pile.pop_back().unwrap();
                    current_player.hand.cards.push(discard_visible);
                    let score = calculate_best_meld_from_hand(&current_player.hand);
                    println!("\nYou played your hand with score: {}", score);
                    return;
                }
                _ => {}
            }
        }

        // Move to next player
        game_state.current_player_idx = (game_state.current_player_idx + 1) % players.len();
    }
}

fn parse_choice(input: &str) -> Result<Choice, String> {
    match input.to_lowercase().as_str() {
        "d" | "draw" => Ok(Choice::Draw),
        "p" | "play" => Ok(Choice::Play),
        "f" | "fold" => Ok(Choice::Fold),
        _ => Err("Invalid input. Expected D (draw) or P (play).".to_string()),
    }
}
