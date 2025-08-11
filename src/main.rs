use std::collections::VecDeque;
use std::io::{self, Write};
use std::path::PathBuf;

use clap::Parser;
use rand::seq::SliceRandom;
use rummy::{analysis::*, card::*, game::*};

use awful_aj::{
    api::ask,
    config::AwfulJadeConfig,
    template::{self, ChatTemplate},
};

#[derive(Debug, Clone)]
struct LayOffResult {
    player_name: String,
    cards_laid_off: Vec<Card>,
    resulting_hand: Hand,
    resulting_score: i32,
    cards_used: usize, // Number of cards from player's hand used
}

#[derive(Debug)]
struct HandResult {
    winner: String,
    final_score: i32,
    winner_hand: Hand,
    lay_offs: Vec<LayOffResult>,
}

struct GameState<'a> {
    book: String,
    players: Vec<Player>,
    deck: &'a mut Deck<'a>,
    actions_log: Vec<String>,
    messages: Vec<String>,
    current_player_idx: usize,
    aj_config: AwfulJadeConfig,
    player_quotes: Vec<String>,
    player_dialogues: std::collections::HashMap<String, String>,
}

/// CLI arguments
#[derive(Parser, Debug)]
#[command(name = "bookclub_rummy")]
#[command(about = "Talk about a book and play 5 Card Rummy", long_about = None)]
struct Args {
    /// Configuration file
    #[arg(short, long)]
    config: PathBuf,
}

impl<'a> GameState<'a> {
    fn clear_screen() {
        print!("\x1B[2J\x1B[1;1H");
        io::stdout().flush().unwrap();
    }

    fn wrap_text(text: &str, line_width: usize, indent: usize) -> String {
        let words: Vec<&str> = text.split_whitespace().collect();
        let mut lines = Vec::new();
        let mut current_line = String::new();

        for word in words {
            if !current_line.is_empty() && current_line.len() + word.len() + 1 > line_width {
                lines.push(current_line.clone());
                current_line.clear();
            }

            if !current_line.is_empty() {
                current_line.push(' ');
            }
            current_line.push_str(word);
        }

        if !current_line.is_empty() {
            lines.push(current_line);
        }

        let indent_str = " ".repeat(indent);
        lines.join(&format!("\n{}", indent_str))
    }

    // Modified to use cached dialogues instead of fetching every time
    async fn display(&self, human_player: &Player, prompt: &str) {
        Self::clear_screen();

        // Header
        let book = &self.book;
        println!("Today's Bookclub Rummy is on {book}\n");

        // Player dialogue with proper wrapping - use cached dialogues
        for player in &self.players {
            if let Some(dialogue) = self.player_dialogues.get(&player.name) {
                let wrapped_dialogue = Self::wrap_text(dialogue, 75, 27);
                println!("{:20} says: {}", player.name, wrapped_dialogue);
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

        // Input prompt BEFORE actions and messages
        print!("{} ", prompt);
        io::stdout().flush().unwrap();

        // Save cursor position after prompt
        print!("\x1B[s");

        // Show recent actions (max 4)
        if !self.actions_log.is_empty() {
            println!("\n\nActions:");
            let start = self.actions_log.len().saturating_sub(4);
            for action in &self.actions_log[start..] {
                println!("{}", action);
            }
        }

        // Show only the most recent message
        if !self.messages.is_empty() {
            println!("\nMessages:");
            println!("{}", self.messages.last().unwrap());
        }

        // Restore cursor position to after the prompt
        print!("\x1B[u");
        io::stdout().flush().unwrap();
    }

    // Add method to update dialogue for current player
    async fn update_current_player_dialogue(&mut self) {
        let current_player = &self.players[self.current_player_idx];
        if current_player.player_type.is_some() {
            let dialogue = self.get_player_dialogue(current_player).await;
            self.player_quotes.push(
                format!("{}: {}", current_player.name, dialogue)
                    .trim()
                    .to_string(),
            );
            self.player_dialogues
                .insert(current_player.name.clone(), dialogue);
        }
    }

    // Add method to display updated state (for end of turn)
    async fn display_updated_state(&self, human_player: &Player) {
        Self::clear_screen();

        // Header
        let book = &self.book;
        println!("Today's Bookclub Rummy is on {book}\n");

        // Player dialogue with proper wrapping
        for player in &self.players {
            if let Some(dialogue) = self.player_dialogues.get(&player.name) {
                let wrapped_dialogue = Self::wrap_text(dialogue, 75, 27);
                println!("{:20} says: {}", player.name, wrapped_dialogue);
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
        println!();

        // Show recent actions (max 4)
        if !self.actions_log.is_empty() {
            println!("\nActions:");
            let start = self.actions_log.len().saturating_sub(4);
            for action in &self.actions_log[start..] {
                println!("{}", action);
            }
        }

        // Brief pause to show the updated state
        tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;
    }

    fn add_message(&mut self, msg: String) {
        self.messages.push(msg);
        // Keep only last message (or last few if you want a short history)
        if self.messages.len() > 1 {
            self.messages.drain(0..self.messages.len() - 1);
        }
    }

    fn add_action(&mut self, player_name: &str, action: &str, card: Option<Card>) {
        let action_text = if let Some(card) = card {
            format!("{:20} {} {}", player_name, action, card)
        } else {
            format!("{:20} {}", player_name, action)
        };
        self.actions_log.push(action_text);
        // Keep only last 4 actions
        if self.actions_log.len() > 4 {
            self.actions_log.remove(0);
        }
    }

    async fn get_player_dialogue(&self, player: &Player) -> String {
        let template = template::load_template("bookclub_rummy").await.unwrap();

        let mut previous_conversation = String::new();
        for (name, quote) in self.player_dialogues.iter() {
            let line = format!("{name}: {quote}");
            previous_conversation = format!("{previous_conversation}\n{line}");
        }

        let book_and_author = &self.book;
        let name = &player.name;
        let description_section = format!(": {}", &player.description);
        let question = format!(
            "Here is the conversation about {book_and_author}\n{previous_conversation}\n\nPlease continue the roleplay by responding with a single sentence. You are playing the role of {name}{description_section}"
        );

        let answer = awful_aj::api::ask(&self.aj_config, question, &template, None, None).await;

        answer.unwrap()
    }

    fn clear_messages(&mut self) {
        self.messages.clear();
    }
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let conf_file = args.config;

    let awful_config = awful_aj::config::load_config(conf_file.to_str().unwrap()).unwrap();

    let mut shuffled_deck = shuffle_deck().unwrap();
    let mut deck = Deck {
        draw_pile: &mut shuffled_deck,
        discard_pile: &mut VecDeque::new(),
    };

    // Step 1: Read number of players from stdin
    println!("Enter the number of players:");
    let mut num_players = String::new();
    io::stdin()
        .read_line(&mut num_players)
        .expect("Failed to read number of players");

    let num_players = match num_players.trim().parse::<usize>() {
        Ok(n) if n >= 2 => n,
        _ => {
            println!(
                "Invalid input. Please enter a number between {} and {}",
                2, 4
            );
            std::process::exit(1);
        }
    };

    // Step 2: Read each player's name
    let mut players = Vec::with_capacity(num_players);
    for i in 0..num_players {
        let name_input = match i {
            0 => "Enter your name:".to_string(),
            _ => format!("Enter name of player {}:", i + 1),
        };
        println!("{name_input}");
        let mut name = String::new();
        io::stdin()
            .read_line(&mut name)
            .expect("Failed to read player name");

        let description = if i != 0 {
            let mut description = String::new();
            println!("Enter player description (Enter if none):");
            io::stdin()
                .read_line(&mut description)
                .expect("Failed to read player description");
            description
        } else {
            "".to_string()
        };

        players.push(Player {
            name: name.trim().to_string(),
            description: description,
            player_type: match i {
                0 => None,
                _ => Some(PlayerType::Balanced),
            },
            hand: Hand { cards: Vec::new() },
            actions: VecDeque::new(),
            dialogue: VecDeque::new(),
        });
    }

    // Step 3: Read Book and Author
    println!("Enter the book and author (East of Eden by John Steinbeck):");
    let mut book_and_author = String::new();
    io::stdin()
        .read_line(&mut book_and_author)
        .expect("Failed to get book and author");

    let mut rng = rand::rng();
    players.shuffle(&mut rng);

    // Deal cards
    let (mut players, deck) = deal_cards(players, &mut deck).unwrap();

    // Flip first discard card
    let visible_discard = deck.draw_pile.pop_back().unwrap();
    deck.discard_pile.push_back(visible_discard);

    let mut game_state = GameState {
        book: book_and_author,
        players: players.clone(),
        deck,
        actions_log: Vec::new(),
        messages: Vec::new(),
        current_player_idx: 0,
        aj_config: awful_config,
        player_quotes: Vec::new(),
        player_dialogues: std::collections::HashMap::new(),
    };

    // Main game loop
    loop {
        let current_player = &mut players[game_state.current_player_idx];

        // Update dialogue for current player at start of their turn
        game_state.update_current_player_dialogue().await;

        // AI player turn
        if let Some(player_type) = current_player.player_type.clone() {
            // Create a hypothetical hand with the discard pile card
            let mut analysis_hand = current_player.hand.clone();
            analysis_hand
                .cards
                .push(*game_state.deck.discard_pile.back().unwrap());

            let baseline_score = calculate_best_meld_from_hand(&analysis_hand);
            let possible_cards: Vec<Card> = game_state.deck.draw_pile.iter().cloned().collect();

            let node = Node {
                full_hand: analysis_hand.clone(), // 6-card hand for analysis
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
                    // ACTUALLY TAKE THE DISCARD PILE CARD!
                    let discard_card = game_state.deck.discard_pile.pop_back().unwrap();
                    current_player.hand.cards.push(discard_card);

                    // Now calculate score with the 6-card hand (should match baseline_score)
                    let score = calculate_best_meld_from_hand(&current_player.hand);
                    game_state.add_action(&current_player.name, "took discard and played!", None);
                    println!(
                        "\n{} played their hand with score: {}",
                        current_player.name, score
                    );
                    return;
                }
                PlayAction::Draw => {
                    // Don't take the discard pile card, draw a new one instead
                    let drawn_card = if let Some(card) = game_state.deck.draw_pile.pop_back() {
                        card
                    } else {
                        game_state.deck.reshuffle_deck();
                        game_state.deck.draw_pile.pop_back().unwrap()
                    };

                    current_player.hand.cards.push(drawn_card);

                    // Find worst card to discard from the actual 6-card hand
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

            // Initial prompt for draw/play decision
            let mut player_choice = None;
            while player_choice.is_none() {
                game_state
                    .display(current_player, "Draw (D) or Play (P)?")
                    .await;

                let mut input = String::new();
                io::stdin()
                    .read_line(&mut input)
                    .expect("Failed to read line");

                match parse_choice(input.trim()) {
                    Ok(choice) => player_choice = Some(choice),
                    Err(err) => {
                        game_state.add_message(err);
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

                    // Ask for discard with updated prompt
                    let mut discard_card = None;
                    while discard_card.is_none() {
                        game_state.clear_messages();
                        game_state
                            .display(current_player, "Which card to discard?")
                            .await;

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
                                }
                            }
                            Err(err) => {
                                game_state.add_message(format!("Invalid card format: {}", err));
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

                    // Ask if they want to play or fold with appropriate prompt
                    let mut final_choice = None;
                    while final_choice.is_none() {
                        game_state
                            .display(current_player, "Play hand (P) or Fold (F)?")
                            .await;

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
                            "f" | "fold" => {
                                final_choice = Some(());
                            }
                            _ => {
                                game_state
                                    .add_message("Please enter P (play) or F (fold)".to_string());
                            }
                        }
                    }
                    game_state
                        .display(current_player, "Join the conversation: ")
                        .await;

                    let mut dialogue = String::new();
                    io::stdin()
                        .read_line(&mut dialogue)
                        .expect("Failed to read line");

                    game_state.player_quotes.push(
                        format!("{}: {}", current_player.name, dialogue)
                            .trim()
                            .to_string(),
                    );
                    game_state
                        .player_dialogues
                        .insert(current_player.name.clone(), dialogue);
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

        // Display updated state at end of turn (before moving to next player)
        let human_player = players.iter().find(|p| p.player_type.is_none()).unwrap();
        game_state.display_updated_state(human_player).await;

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
