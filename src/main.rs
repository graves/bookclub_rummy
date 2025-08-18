use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::io::{self, Write};
use std::path::PathBuf;

use clap::Parser;
use rand::Rng;
use rand::seq::SliceRandom;
use terminal_size::{Width, terminal_size};

use rummy::{analysis::*, card::*, game::*};

use awful_aj::{
    config::AwfulJadeConfig,
    template::{self},
};

#[derive(Debug, Clone)]
struct LayOffResult {
    player: Player,
    cards_laid_off: Vec<Card>,
    resulting_hand: Hand,
    resulting_score: u64,
    cards_used: usize,
}

// Create a wrapper to own the deck data
struct DeckData {
    draw_pile: VecDeque<Card>,
    discard_pile: VecDeque<Card>,
}

struct ColoredName {
    name: String,
    color_code: String,
}

impl ColoredName {
    fn new(name: String, player_index: usize) -> Self {
        // Colors that work well on both dark and light backgrounds
        // Using the 256-color palette for better visibility
        let safe_colors = [
            "38;5;33",  // Blue
            "38;5;127", // Purple
            "38;5;166", // Orange
            "38;5;28",  // Green
            "38;5;124", // Red/Maroon
            "38;5;94",  // Brown
            "38;5;31",  // Teal
            "38;5;130", // Dark Orange
        ];

        // Cycle through colors if more players than colors
        let color = safe_colors[player_index % safe_colors.len()];

        Self {
            name,
            color_code: color.to_string(),
        }
    }

    fn colored(&self) -> String {
        format!("\x1B[{}m{}\x1B[0m", self.color_code, self.name)
    }

    fn colored_padded(&self, width: usize) -> String {
        // Account for ANSI codes when padding
        let visible_len = self.name.len();
        let padding = " ".repeat(width.saturating_sub(visible_len));
        format!("\x1B[{}m{}{}\x1B[0m", self.color_code, self.name, padding)
    }
}

impl DeckData {
    fn new(cards: Vec<Card>) -> Self {
        Self {
            draw_pile: cards.into_iter().collect(),
            discard_pile: VecDeque::new(),
        }
    }

    fn reshuffle(&mut self) {
        // Keep the top card of discard pile
        let top_card = self.discard_pile.pop_back();

        // Move all other discard cards to draw pile
        self.draw_pile.extend(self.discard_pile.drain(..));

        // Shuffle the draw pile
        let mut cards: Vec<Card> = self.draw_pile.drain(..).collect();
        let mut rng = rand::rng();
        cards.shuffle(&mut rng);
        self.draw_pile = cards.into_iter().collect();

        // Put the top card back
        if let Some(card) = top_card {
            self.discard_pile.push_back(card);
        }
    }
}

struct GameState {
    book: String,
    players: RefCell<Vec<Player>>,
    player_colors: Vec<ColoredName>,
    deck: RefCell<DeckData>,
    actions_log: RefCell<Vec<String>>,
    messages: RefCell<Vec<String>>,
    current_player_idx: RefCell<usize>,
    aj_config: AwfulJadeConfig,
    player_quotes: RefCell<Vec<String>>,
    player_dialogues: RefCell<HashMap<String, String>>,
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

impl GameState {
    fn clear_screen() {
        print!("\x1B[2J\x1B[1;1H");
        io::stdout().flush().unwrap();
    }

    fn actions(&self) -> Vec<String> {
        let mut revd = self.actions_log.borrow().clone();
        revd.reverse();
        revd
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
        lines.join(&format!("\n{indent_str}"))
    }

    fn get_player_color(&self, player_name: &str) -> Option<&ColoredName> {
        self.player_colors.iter().find(|cn| cn.name == player_name)
    }

    fn display_dialogues(&self) {
        const MAX_LINES: usize = 11; // size of dialogue field

        let log = self.player_quotes.borrow();
        let start = log.len().saturating_sub(MAX_LINES);
        let recent = &log[start..];

        // pad blank lines at top if not enough
        for _ in 0..(MAX_LINES - recent.len()) {
            println!();
        }

        // print the recent lines (already wrapped/indented)
        for line in recent {
            println!("{line}");
        }
    }

    fn push_dialogue(&self, player: &Player, dialogue: &str) {
        // Visible layout constants
        const NAME_COLS: usize = 20; // matches your padded name field
        const SAYS: &str = " says: ";
        const PREFIX_VIS_COLS: usize = NAME_COLS + SAYS.len();
        const CONTENT_COLS: usize = 75; // width of the dialogue text area

        // Get the colored name, already padded to NAME_COLS visible columns
        let colored = self.get_player_color(&player.name).unwrap();
        let name_field_colored = colored.colored_padded(NAME_COLS);

        // 1) Wrap the *raw* dialogue to the content width with NO indent
        //    (avoid double-indenting)
        let wrapped = Self::wrap_text(dialogue, CONTENT_COLS, 0);

        // 2) Emit the first line with the colored prefix
        let mut lines = Vec::new();
        let mut iter = wrapped.lines();
        if let Some(first) = iter.next() {
            lines.push(format!(
                "{}{}{}",
                name_field_colored,
                SAYS,
                Self::colorize_text(first, &colored.color_code)
            ));
        }

        // 3) Emit continuation lines: indent exactly to the *visible* prefix column
        for cont in iter {
            lines.push(format!(
                "{:width$}{}",
                "",
                Self::colorize_text(cont, &colored.color_code),
                width = PREFIX_VIS_COLS
            ));
        }

        self.player_quotes.borrow_mut().extend(lines);
    }

    async fn display(&self, human_player: &Player, prompt: &str) {
        Self::clear_screen();

        println!("{}\n", self.colored_book_title());

        self.display_dialogues();
        println!();

        let deck = self.deck.borrow();
        if let Some(top_card) = deck.discard_pile.back() {
            println!("[{top_card}] [⌧]");
        } else {
            println!("[--] [⌧]");
        }

        print!(" ");
        for card in &human_player.hand.cards {
            print!("{card} ");
        }
        println!("\n");

        print!("{prompt} ");
        io::stdout().flush().unwrap();

        print!("\x1B[s");

        if !self.actions_log.borrow().is_empty() {
            println!("\n\nActions:");
            let start = self.actions_log.borrow().len().saturating_sub(6);
            for action in &self.actions()[start..] {
                println!("{action}");
            }
        }

        // Color the names in the Scoreboard
        println!("\n\nScoreboard:");
        for player in self.players.borrow().iter() {
            if let Some(colored_name) = self.get_player_color(&player.name) {
                println!("{}: {}", colored_name.colored(), player.score);
            } else {
                println!("{}: {}", player.name, player.score);
            }
        }

        if !self.messages.borrow().is_empty() {
            println!("\nMessages:");
            println!("{}", self.messages.borrow().last().unwrap());
        }

        print!("\x1B[u");
        io::stdout().flush().unwrap();
    }

    async fn display_layoff(&self, human_player: &Player, hand_player: &Player, prompt: &str) {
        Self::clear_screen();

        println!("{}\n", self.colored_book_title());

        self.display_dialogues();

        println!();

        println!("[--] [⌧]");

        // Color the player names in the hand display
        print!(" ");
        if let Some(colored_name) = self.get_player_color(&hand_player.name) {
            print!("{} hand: ", colored_name.colored_padded(20));
        } else {
            let name = format!("{}'s", hand_player.name);
            print!("{name:20} hand: ");
        }
        for card in &hand_player.hand.cards {
            print!("{card} ");
        }
        println!();

        print!(" ");
        if let Some(colored_name) = self.get_player_color(&human_player.name) {
            print!("{} hand: ", colored_name.colored_padded(20));
        } else {
            let name = format!("{}'s", human_player.name);
            print!("{name:20} hand: ");
        }
        for card in &human_player.hand.cards {
            print!("{card} ");
        }
        println!("\n");

        print!("{prompt} ");
        io::stdout().flush().unwrap();

        print!("\x1B[s");

        if !self.actions_log.borrow().is_empty() {
            println!("\n\nActions:");
            let start = self.actions_log.borrow().len().saturating_sub(6);
            for action in &self.actions()[start..] {
                println!("{action}");
            }
        }

        // Color the names in the Scoreboard
        println!("\n\nScoreboard:");
        for player in self.players.borrow().iter() {
            if let Some(colored_name) = self.get_player_color(&player.name) {
                println!("{}: {}", colored_name.colored(), player.score);
            } else {
                println!("{}: {}", player.name, player.score);
            }
        }

        if !self.messages.borrow().is_empty() {
            println!("\nMessages:");
            println!("{}", self.messages.borrow().last().unwrap());
        }

        print!("\x1B[u");
        io::stdout().flush().unwrap();
    }

    async fn update_current_player_dialogue(&self) {
        let current_idx = *self.current_player_idx.borrow();
        let current_player = self.players.borrow()[current_idx].clone();
        if current_player.player_type.is_some() {
            let dialogue = self.get_player_dialogue(&current_player).await;
            self.push_dialogue(&current_player, dialogue.trim());
            self.player_dialogues
                .borrow_mut()
                .insert(current_player.name.clone(), dialogue.clone());
        }
    }

    async fn display_updated_state(&self, human_player: &Player) {
        Self::clear_screen();

        println!("{}\n", self.colored_book_title());

        self.display_dialogues();

        println!();

        let deck = self.deck.borrow();
        if let Some(top_card) = deck.discard_pile.back() {
            println!("[{top_card}] [⌧]");
        } else {
            println!("[--] [⌧]");
        }

        print!(" ");
        for card in &human_player.hand.cards {
            print!("{card} ");
        }
        println!();

        if !self.actions_log.borrow().is_empty() {
            println!("\nActions:");
            let start = self.actions_log.borrow().len().saturating_sub(6);
            for action in &self.actions()[start..] {
                println!("{action}");
            }
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;
    }

    fn add_message(&self, msg: String) {
        self.messages.borrow_mut().push(msg);
        let len = self.messages.borrow().len();
        if len > 1 {
            self.messages.borrow_mut().drain(0..len - 1);
        }
    }

    fn add_action(&self, player_name: &str, action: &str, card: Option<Card>) {
        let colored_name = self
            .player_colors
            .iter()
            .find(|cn| cn.name == player_name)
            .map(|cn| cn.colored_padded(20))
            .unwrap_or_else(|| format!("{player_name:20}"));

        let color_code = self
            .player_colors
            .iter()
            .find(|cn| cn.name == player_name)
            .map(|cn| cn.color_code.clone())
            .unwrap_or_else(|| "0".to_string());

        // Check if action contains cards
        let contains_cards = action.contains("♢")
            || action.contains("♡")
            || action.contains("♤")
            || action.contains("♧");

        let action_text = if contains_cards {
            // For "played their hand X X X X X for Y points" format
            if action.contains("played their hand") && action.contains(" for ") {
                // Find the positions
                let hand_start =
                    action.find("played their hand").unwrap() + "played their hand".len();
                let for_pos = action.rfind(" for ").unwrap();

                // Split into parts
                let prefix = &action[..hand_start]; // "played their hand"
                let cards = &action[hand_start..for_pos]; // " 9♧ 7♧ Q♤ 9♤ 7♤"
                let suffix = &action[for_pos..]; // " for 2 points. It's time to layoff."

                // TODO cards is blank when the human player plays and an extra blank space shows after "for"
                // Color the non-card parts
                format!(
                    "{colored_name} \x1B[{color_code}m{prefix}\x1B[0m{cards}\x1B[{color_code}m{suffix}\x1B[0m"
                )
            } else if action.contains("won this round") && action.contains("hand ") {
                // Find the positions
                let hand_start = action.find("won this round with a score of").unwrap()
                    + "won this round with the score of 2 and the hand".len();

                // Split into parts
                let prefix = &action[..hand_start]; // "won this round with the score of and the hand"
                let cards = &action[hand_start..]; // " 9♧ 7♧ Q♤ 9♤ 7♤"

                // Color the nonjjj
                format!("{colored_name} \x1B[{color_code}m{prefix}\x1B[0m{cards}")
            } else {
                // Other card-containing actions - just color the name
                format!("{colored_name} {action}")
            }
        } else if let Some(card) = card {
            // Single card action
            format!("{colored_name} \x1B[{color_code}m{action}\x1B[0m {card}")
        } else {
            // No cards
            format!("{colored_name} \x1B[{color_code}m{action}\x1B[0m")
        };

        self.actions_log.borrow_mut().push(action_text);
        if self.actions_log.borrow().len() > 6 {
            self.actions_log.borrow_mut().remove(0);
        }
    }

    async fn get_player_dialogue(&self, player: &Player) -> String {
        let template = template::load_template("bookclub_rummy").await.unwrap();

        let mut previous_conversation = String::new();
        for (name, quote) in self.player_dialogues.borrow().iter() {
            let line = format!("{name}: {quote}");
            previous_conversation = format!("{previous_conversation}\n{line}");
        }

        let book_and_author = &self.book;
        let name = &player.name;
        let description_section = format!(": {}", &player.description);
        let question = format!(
            "Here is the conversation about {book_and_author}\n{previous_conversation}\n\nPlease continue the roleplay by responding with a single sentence. Always end the sentence with an emoji representing your emotional state. Please ensure you are responding directly to another player's previous dialogue. You are playing the role of {name}{description_section}"
        );

        let answer = awful_aj::api::ask(&self.aj_config, question, &template, None, None)
            .await
            .unwrap();

        let name_prompt = format!("{name}: ");

        answer.replace(&name_prompt, "")
    }

    fn clear_messages(&self) {
        self.messages.borrow_mut().clear();
    }

    fn prompt_for_layoff_cards(
        &self,
        human_player: &Player,
        hand_player: &Player,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Vec<Card>> + '_>> {
        let human_player = human_player.clone();
        let hand_player = hand_player.clone();

        Box::pin(async move {
            self.display_layoff(
                &human_player,
                &hand_player,
                "Enter cards to lay off separated by spaces (e.g. \"7h Jc\"): ",
            )
            .await;

            let mut input = String::new();
            io::stdout().flush().unwrap();
            io::stdin().read_line(&mut input).unwrap();
            let trimmed = input.trim();
            if trimmed.is_empty() {
                return vec![];
            }

            let mut chosen = Vec::new();
            for token in trimmed.split_whitespace() {
                let card = Card::from_string(token.to_string());
                match card {
                    Ok(card) => {
                        if human_player.hand.cards.contains(&card) {
                            self.clear_messages();
                            chosen.push(card)
                        } else {
                            self.add_message(format!("You don't have {token}"));
                            return self
                                .prompt_for_layoff_cards(&human_player, &hand_player)
                                .await;
                        }
                    }
                    _ => {
                        self.add_message("Invalid card!".to_string());
                        return self
                            .prompt_for_layoff_cards(&human_player, &hand_player)
                            .await;
                    }
                }
            }

            chosen
        })
    }

    fn update_scores(&self, player: &Player, score: usize) {
        self.players.borrow_mut().iter_mut().for_each(|p| {
            if p == player {
                p.score += score
            }
        });
    }

    fn deal_new_round(&self) {
        // Clear hands
        for player in self.players.borrow_mut().iter_mut() {
            player.hand.cards.clear();
        }

        // Get all cards from deck and reshuffle
        let mut all_cards = Vec::new();
        all_cards.extend(self.deck.borrow_mut().draw_pile.drain(..));
        all_cards.extend(self.deck.borrow_mut().discard_pile.drain(..));

        // If not enough cards, create a new deck
        if all_cards.len() < 52 {
            all_cards = shuffle_deck().unwrap().into();
        }

        let mut rng = rand::rng();
        all_cards.shuffle(&mut rng);

        // Deal 5 cards to each player
        for player in self.players.borrow_mut().iter_mut() {
            for _ in 0..5 {
                if let Some(card) = all_cards.pop() {
                    player.hand.cards.push(card);
                }
            }
        }

        // Put remaining cards in draw pile
        self.deck.borrow_mut().draw_pile = all_cards.into_iter().collect();

        // Turn over one card for discard pile
        let mut deck = self.deck.borrow_mut();
        if let Some(card) = deck.draw_pile.pop_back() {
            deck.discard_pile.push_back(card);
        }
    }

    // Helper function to colorize multi-line text
    fn colorize_text(text: &str, color_code: &str) -> String {
        let lines: Vec<String> = text
            .lines()
            .map(|line| format!("\x1B[{color_code}m{line}\x1B[0m"))
            .collect();
        lines.join("\n")
    }

    fn colored_book_title(&self) -> String {
        // Pastel green (using 256-color palette) + bold
        format!(
            "\x1B[1;38;5;120mToday's Bookclub Rummy is on {}\x1B[0m",
            self.book
        )

        // Alternative pastel green options:
        // format!("\x1B[1;38;5;114m...")  // Slightly different pastel green
        // format!("\x1B[1;38;5;156m...")  // Very light pastel green
        // format!("\x1B[1;92m...")        // Basic bright green (16-color)
    }

    async fn display_victory_animation(&self, winner_name: &str) {
        let angel = vec![
            "               ______",
            "              '-._   ```\"\"\"---.._",
            "           ,-----.:___           `\\  ,;;;,",
            "            '-.._     ```\"\"\"--.._  |,%%%%%%              _",
            "            ,    '.              `\\;;;;  -\\      _    _.'/\\",
            "          .' `-.__ \\            ,;;;;\" .__{=====/_)==:_  ||",
            "     ,===/        ```\";,,,,,,,;;;;;'`-./.____,'/ /     '.\\/",
            "    '---/              ';;;;;;;;'      `--.._.' /",
            "   ,===/                          '-.        `\\/",
            "  '---/                            ,'`.        |",
            "     ;                        __.-'    \\     ,'",
            "jgs  \\______,,.....------'''``          `---`",
        ];

        // Get terminal width
        let term_width = terminal_size()
            .map(|(Width(w), _)| w as usize)
            .unwrap_or(80);
        let angel_width = angel.iter().map(|line| line.len()).max().unwrap_or(0);

        // Get winner's color
        let winner_color = self
            .player_colors
            .iter()
            .find(|cn| cn.name == winner_name)
            .map(|cn| cn.color_code.clone())
            .unwrap_or_else(|| "0".to_string());

        // Create RNG instance
        let mut rng = rand::rng();

        // Phase 1: Angel glides from left to right
        for position in (0..=(term_width.saturating_sub(angel_width))).step_by(2) {
            Self::clear_screen();

            // Rainbow colors for victory message
            let colors = ["31", "33", "32", "36", "34", "35"];
            let victory_msg = "VICTORY!";
            print!("{:^width$}", "", width = term_width / 2 - 4);
            for (i, ch) in victory_msg.chars().enumerate() {
                print!("\x1B[1;{}m{}\x1B[0m", colors[i % colors.len()], ch);
            }
            println!("\n");

            println!(
                "{:^width$}\x1B[1;{}m{} wins!\x1B[0m\n",
                "",
                winner_color,
                winner_name,
                width = term_width
            );

            // Display the angel with gradient effect
            for (i, line) in angel.iter().enumerate() {
                let color = if i < angel.len() / 2 { "229" } else { "231" };
                println!(
                    "{:width$}\x1B[38;5;{}m{}\x1B[0m",
                    "",
                    color,
                    line,
                    width = position
                );
            }

            // Trailing sparkles
            let sparkles = ["✨", "⭐", "✦", "✧", "⋆"];
            for i in 0..5 {
                if position > (i + 1) * 8 {
                    let sparkle_pos = position - (i + 1) * 8;
                    let sparkle_line = 5 + i;
                    if sparkle_line < angel.len() {
                        print!("\x1B[{}A", angel.len() - sparkle_line);
                        println!(
                            "{:width$}\x1B[38;5;226m{}\x1B[0m",
                            "",
                            sparkles[i % sparkles.len()],
                            width = sparkle_pos
                        );
                        print!("\x1B[{}B", angel.len() - sparkle_line - 1);
                    }
                }
            }

            io::stdout().flush().unwrap();
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }

        // Phase 2: Final celebration with confetti
        for frame in 0..3 {
            Self::clear_screen();

            // Random confetti
            let confetti = ["🎉", "🎊", "🌟", "✨", "🎈"];
            for _ in 0..10 {
                let x = rng.random_range(0..term_width);
                let y = rng.random_range(0..5);
                let confetti_char = confetti[rng.random_range(0..confetti.len())];
                print!("\x1B[{};{}H{}", y + 1, x + 1, confetti_char);
            }

            println!("\n\n\n\n\n");

            // Victory message with pulsing effect
            let size = if frame % 2 == 0 { "1" } else { "1;5" };
            println!(
                "{:^width$}\x1B[{};{}m{} WINS THE GAME!\x1B[0m",
                "",
                size,
                winner_color,
                winner_name.to_uppercase(),
                width = term_width
            );

            println!("\n");

            // Center the angel
            let center_pos = (term_width - angel_width) / 2;
            for line in &angel {
                println!(
                    "{:width$}\x1B[38;5;229m{}\x1B[0m",
                    "",
                    line,
                    width = center_pos
                );
            }

            io::stdout().flush().unwrap();
            tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
        }

        let msg = format!(
            "\n\n{:^width$}\x1B[2mPress Enter to exit...\x1B[0m",
            "",
            width = term_width
        );
        let mut input = String::new();
        io::stdout().write_all(msg.as_bytes()).unwrap();
        io::stdout().flush().unwrap();

        if io::stdin().read_line(&mut input).is_err() {
            panic!("Couldn't read from stdin!");
        } else if input.trim() == "" {
            std::process::exit(0);
        }
    }
}

async fn run_layoff_round(
    winner_idx: usize,
    winner_hand: &Hand,
    players: &[Player],
    score_to_beat: u64,
    game_state: &GameState,
) -> (Option<Vec<LayOffResult>>, Hand) {
    let mut score_to_beat = score_to_beat;
    let mut lay_off_results = Vec::new();
    let mut all_layoff_cards = Vec::new();
    let num_players = players.len();

    game_state.add_action(
        &players[winner_idx].name,
        &format!(
            "played their hand {winner_hand} for {score_to_beat} points. It's time to layoff."
        ),
        None,
    );

    let mut current_idx = (winner_idx + 1) % num_players;
    let mut players = players.to_owned();
    let mut winner_hand = winner_hand.clone();
    let mut layoff_winner_idx = winner_idx;

    while current_idx != winner_idx {
        let is_human = players[current_idx].player_type.is_none();
        if is_human {
            let chosen_cards = game_state
                .prompt_for_layoff_cards(&players[current_idx], &players[layoff_winner_idx])
                .await;

            if !chosen_cards.is_empty() {
                // Remove chosen cards from player's hand
                for card in &chosen_cards {
                    if let Some(pos) = players[current_idx]
                        .hand
                        .cards
                        .iter()
                        .position(|c| c == card)
                    {
                        players[current_idx].hand.cards.remove(pos);
                    }
                }

                let mut best_layoff: Option<LayOffResult> = None;
                let mut best_score = score_to_beat;

                if chosen_cards.len() == 1 {
                    // For single card layoff, try replacing each card in winner's hand
                    for i in 0..winner_hand.cards.len() {
                        let mut test_hand = winner_hand.clone();
                        test_hand.cards[i] = chosen_cards[0];

                        let (test_score, new_hand) =
                            calculate_best_meld_from_5_card_hand(&test_hand);

                        if test_score > best_score {
                            best_score = test_score;
                            best_layoff = Some(LayOffResult {
                                player: players[current_idx].clone(),
                                cards_laid_off: chosen_cards.clone(),
                                resulting_hand: new_hand,
                                resulting_score: test_score,
                                cards_used: 1,
                            });
                        }
                    }
                } else if chosen_cards.len() == 2 {
                    // For two card layoff, try all combinations of replacing 2 cards
                    for i in 0..(winner_hand.cards.len() - 1) {
                        for j in (i + 1)..winner_hand.cards.len() {
                            let mut test_hand = winner_hand.clone();
                            test_hand.cards[i] = chosen_cards[0];
                            test_hand.cards[j] = chosen_cards[1];

                            let (test_score, new_hand) =
                                calculate_best_meld_from_5_card_hand(&test_hand);

                            if test_score > best_score {
                                best_score = test_score;
                                best_layoff = Some(LayOffResult {
                                    player: players[current_idx].clone(),
                                    cards_laid_off: chosen_cards.clone(),
                                    resulting_hand: new_hand,
                                    resulting_score: test_score,
                                    cards_used: 2,
                                });
                            }
                        }
                    }
                }

                if let Some(layoff) = best_layoff {
                    winner_hand = layoff.resulting_hand.clone();
                    all_layoff_cards.extend(chosen_cards.clone());
                    score_to_beat = layoff.resulting_score;

                    game_state.add_action(
                        &players[current_idx].name,
                        &format!(
                            "laid off {} card(s) to winner's meld, scoring: {}",
                            chosen_cards.len(),
                            score_to_beat
                        ),
                        None,
                    );

                    lay_off_results.push(layoff);
                } else {
                    // Put cards back in player's hand if layoff failed
                    players[current_idx].hand.cards.extend(chosen_cards.clone());

                    game_state.add_action(
                        &players[current_idx].name,
                        "could not layoff cards to form a meld.",
                        None,
                    );
                }
            }
        } else {
            // AI layoff logic
            if let Some(layoff) =
                check_for_layoff(&players[current_idx], &winner_hand, score_to_beat)
            {
                winner_hand.cards = layoff.resulting_hand.cards.clone();
                all_layoff_cards.extend(layoff.cards_laid_off.clone());

                score_to_beat = layoff.resulting_score;

                game_state.add_action(
                    &players[current_idx].name,
                    &format!(
                        "laid off {} card(s) to winner's meld, scoring: {}",
                        layoff.cards_used, score_to_beat
                    ),
                    None,
                );

                winner_hand = layoff.resulting_hand.clone();
                lay_off_results.push(layoff);
                players[current_idx].hand = winner_hand.clone();
                layoff_winner_idx = current_idx;
            }
        }

        current_idx = (current_idx + 1) % num_players;
    }

    let lay_off_results = if lay_off_results.is_empty() {
        None
    } else {
        Some(lay_off_results)
    };

    (lay_off_results, winner_hand)
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let conf_file = args.config;

    let awful_config = awful_aj::config::load_config(conf_file.to_str().unwrap()).unwrap();

    let shuffled_deck = shuffle_deck().unwrap();

    println!("\x1B[1;38;5;120mEnter number of players:\x1B[0m");
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

    let mut players = Vec::with_capacity(num_players);
    for i in 0..num_players {
        let name_input = match i {
            0 => "\x1B[1;38;5;120mEnter your name:\x1B[0m".to_string(),
            _ => format!("\x1B[1;38;5;120mEnter name of player {}:\x1B[0m:", i + 1),
        };
        println!("{name_input}");
        let mut name = String::new();
        io::stdin()
            .read_line(&mut name)
            .expect("Failed to read player name");

        let description = if i != 0 {
            let mut description = String::new();
            println!("\x1B[1;38;5;120mEnter player description (Press enter if none):\x1B[0m");
            io::stdin()
                .read_line(&mut description)
                .expect("Failed to read player description");
            description
        } else {
            "".to_string()
        };

        players.push(Player {
            name: name.trim().to_string(),
            description,
            player_type: match i {
                0 => None,
                _ => Some(PlayerType::Balanced),
            },
            hand: Hand { cards: Vec::new() },
            actions: VecDeque::new(),
            dialogue: VecDeque::new(),
            score: 0,
        });
    }

    println!("\x1B[1;38;5;120mEnter book and author (East of Eden by John Steinbeck)\x1B[0m");
    let mut book_and_author = String::new();
    io::stdin()
        .read_line(&mut book_and_author)
        .expect("Failed to get book and author");

    let mut rng = rand::rng();
    players.shuffle(&mut rng);

    // Initialize deck data
    let deck_data = DeckData::new(shuffled_deck.into());

    // Create colored names for each player
    let player_colors: Vec<ColoredName> = players
        .iter()
        .enumerate()
        .map(|(idx, player)| ColoredName::new(player.name.clone(), idx))
        .collect();

    let game_state = GameState {
        book: book_and_author,
        players: RefCell::new(players.clone()),
        player_colors,
        deck: RefCell::new(deck_data),
        actions_log: RefCell::new(Vec::new()),
        messages: RefCell::new(Vec::new()),
        current_player_idx: RefCell::new(0),
        aj_config: awful_config,
        player_quotes: RefCell::new(Vec::new()),
        player_dialogues: RefCell::new(HashMap::new()),
    };

    // Initial deal
    game_state.deal_new_round();

    loop {
        let winner = winning_player(&game_state);

        if let Some(winning_player) = winner {
            if winning_player.player_type.is_none() {
                game_state
                    .display_victory_animation(&winning_player.name)
                    .await;
            } else {
                println!("\n{} won todays Bookclub Rummy!", winning_player.name);
                println!("\n\nFinal Scores:");
                for player in game_state.players.borrow().iter() {
                    if let Some(colored_name) = game_state.get_player_color(&player.name) {
                        println!("{}: {}", colored_name.colored(), player.score);
                    } else {
                        println!("{}: {}", player.name, player.score);
                    }
                }
                std::process::exit(0);
            }
        }

        let current_idx = *game_state.current_player_idx.borrow();
        // Get current player from game_state, not from local players array
        let mut current_player = game_state.players.borrow()[current_idx].clone();

        game_state.update_current_player_dialogue().await;

        if let Some(player_type) = current_player.player_type.clone() {
            // AI Player Turn
            let possible_cards: Vec<Card> =
                game_state.deck.borrow().draw_pile.iter().cloned().collect();
            let discard_card = *game_state.deck.borrow().discard_pile.back().unwrap();

            let mut retrieve_hand = current_player.hand.clone();
            retrieve_hand.cards.push(discard_card);

            let (baseline_score, _hand) = calculate_best_meld_from_hand(&retrieve_hand);

            let retrieve_node = Node {
                full_hand: retrieve_hand.clone(),
                possible_hands: Vec::new(),
                possible_cards: possible_cards.clone(),
                discard_pile: game_state.deck.borrow().discard_pile.clone(),
                meld_score: None,
                baseline_score,
                branches: Vec::new(),
                depth: 0,
            };

            let retrieve_prob_analysis = retrieve_node.calculate_cumulative_probabilities();
            let retrieve_decision =
                retrieve_node.make_autoplay_decision(player_type.clone(), &retrieve_prob_analysis);

            let mut total_draw_score = 0.0;
            let mut draw_scenarios = 0;

            for &possible_draw_card in &possible_cards {
                let mut draw_hand = current_player.hand.clone();
                let (baseline_score, _hand) = calculate_best_meld_from_hand(&draw_hand);
                draw_hand.cards.push(possible_draw_card);

                let draw_node = Node {
                    full_hand: draw_hand.clone(),
                    possible_hands: Vec::new(),
                    possible_cards: possible_cards.clone(),
                    discard_pile: game_state.deck.borrow().discard_pile.clone(),
                    meld_score: None,
                    baseline_score,
                    branches: Vec::new(),
                    depth: 0,
                };

                let prob_analysis = draw_node.calculate_cumulative_probabilities();
                let decision =
                    draw_node.make_autoplay_decision(player_type.clone(), &prob_analysis);

                total_draw_score += decision.expected_score;
                draw_scenarios += 1;
            }

            let average_draw_score = if draw_scenarios > 0 {
                total_draw_score / draw_scenarios as f64
            } else {
                0.0
            };

            let draw_decision = AutoPlayDecision {
                action: PlayAction::Draw,
                confidence: 0.5,
                expected_score: average_draw_score,
                card_to_discard: None,
            };

            let final_decision = if retrieve_decision.expected_score > draw_decision.expected_score
            {
                if retrieve_decision.action == PlayAction::Play {
                    AutoPlayDecision {
                        action: PlayAction::Play,
                        confidence: retrieve_decision.confidence,
                        expected_score: retrieve_decision.expected_score,
                        card_to_discard: None,
                    }
                } else {
                    AutoPlayDecision {
                        action: PlayAction::Retrieve,
                        confidence: retrieve_decision.confidence,
                        expected_score: retrieve_decision.expected_score,
                        card_to_discard: retrieve_decision.card_to_discard,
                    }
                }
            } else {
                draw_decision
            };

            match final_decision.action {
                PlayAction::Play => {
                    let (score, melded_hand) =
                        calculate_best_meld_from_5_card_hand(&current_player.hand);

                    game_state.add_message(format!(
                        "{} played their hand with score: {}",
                        &current_player.name, score
                    ));

                    // Use game_state.players for layoff round
                    let layoff_players = game_state.players.borrow().clone();

                    let (lay_offs, _winning_hand) = run_layoff_round(
                        current_idx,
                        &melded_hand, // Pass the melded hand, not the original
                        &layoff_players,
                        score,
                        &game_state,
                    )
                    .await;

                    if let Some(mut lay_offs) = lay_offs {
                        lay_offs.sort_by(|a, b| b.resulting_score.cmp(&a.resulting_score));
                        let winning_lay_off = lay_offs[0].clone();

                        let layoff_score = if winning_lay_off.cards_used == 2 {
                            0
                        } else {
                            winning_lay_off.resulting_score
                        };

                        game_state.add_action(
                            &winning_lay_off.player.name,
                            &format!(
                                "won this round with a score of {} and the hand {}",
                                &layoff_score, &winning_lay_off.resulting_hand
                            ),
                            None,
                        );

                        game_state.update_scores(&winning_lay_off.player, layoff_score as usize);
                    } else {
                        game_state.add_action(
                            &current_player.name,
                            &format!(
                                "won this round with a score of {} and the hand {}",
                                score,
                                &melded_hand // Use melded_hand here too
                            ),
                            None,
                        );

                        // Update score using current_player reference
                        game_state.update_scores(&current_player, score as usize);
                    }

                    // Deal new round after someone wins
                    game_state.deal_new_round();
                }
                PlayAction::Draw => {
                    let drawn_card =
                        if let Some(card) = game_state.deck.borrow_mut().draw_pile.pop_back() {
                            card
                        } else {
                            game_state.deck.borrow_mut().reshuffle();
                            game_state.deck.borrow_mut().draw_pile.pop_back().unwrap()
                        };

                    current_player.hand.cards.push(drawn_card);
                    let (baseline_score, _hand) =
                        calculate_best_meld_from_hand(&current_player.hand);

                    let actual_hand = current_player.hand.clone();
                    let node = Node {
                        full_hand: actual_hand,
                        possible_hands: Vec::new(),
                        possible_cards: game_state
                            .deck
                            .borrow()
                            .draw_pile
                            .iter()
                            .cloned()
                            .collect(),
                        discard_pile: game_state.deck.borrow().discard_pile.clone(),
                        meld_score: None,
                        baseline_score,
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
                    game_state
                        .deck
                        .borrow_mut()
                        .discard_pile
                        .push_back(discarded);

                    game_state.add_action(
                        &current_player.name,
                        "drew and discarded the",
                        Some(discarded),
                    );

                    // Update the player in game_state
                    game_state.players.borrow_mut()[current_idx] = current_player.clone();
                }
                PlayAction::Retrieve => {
                    let discard_card = game_state
                        .deck
                        .borrow_mut()
                        .discard_pile
                        .pop_back()
                        .unwrap();
                    current_player.hand.cards.push(discard_card);

                    let (baseline_score, _hand) =
                        calculate_best_meld_from_hand(&current_player.hand);

                    let node = Node {
                        full_hand: current_player.hand.clone(),
                        possible_hands: Vec::new(),
                        possible_cards: game_state
                            .deck
                            .borrow()
                            .draw_pile
                            .iter()
                            .cloned()
                            .collect(),
                        discard_pile: game_state.deck.borrow().discard_pile.clone(),
                        meld_score: None,
                        baseline_score,
                        branches: Vec::new(),
                        depth: 0,
                    };

                    let worst_card = node.find_worst_card_to_discard();
                    let idx = current_player
                        .hand
                        .cards
                        .iter()
                        .position(|c| *c == worst_card)
                        .unwrap();
                    let discarded = current_player.hand.cards.remove(idx);
                    game_state
                        .deck
                        .borrow_mut()
                        .discard_pile
                        .push_back(discarded);

                    game_state.add_action(
                        &current_player.name,
                        "retrieved discard and discarded the",
                        Some(discarded),
                    );

                    // Update the player in game_state
                    game_state.players.borrow_mut()[current_idx] = current_player.clone();
                }
            }

            players[current_idx] = current_player;
        } else {
            // Human player turn
            let mut player_choice = None;
            while player_choice.is_none() {
                game_state
                    .display(&current_player, "Draw (D), Play (P), or Retrieve (R)?")
                    .await;

                let mut input = String::new();
                io::stdin()
                    .read_line(&mut input)
                    .expect("Failed to read line");

                match parse_choice(input.trim()) {
                    Ok(choice) => {
                        game_state.clear_messages();
                        player_choice = Some(choice)
                    }
                    Err(err) => {
                        game_state.add_message(err);
                    }
                }
            }

            match player_choice.unwrap() {
                Choice::Draw => {
                    let drawn_card =
                        if let Some(card) = game_state.deck.borrow_mut().draw_pile.pop_back() {
                            card
                        } else {
                            game_state.deck.borrow_mut().reshuffle();
                            game_state.deck.borrow_mut().draw_pile.pop_back().unwrap()
                        };

                    current_player.hand.cards.push(drawn_card);

                    let mut discard_card = None;
                    while discard_card.is_none() {
                        game_state
                            .display(&current_player, "Which card to discard?")
                            .await;

                        let mut input = String::new();
                        io::stdin()
                            .read_line(&mut input)
                            .expect("Failed to read line");

                        match Card::from_string(input.trim().to_string()) {
                            Ok(card) => {
                                if current_player.hand.cards.contains(&card) {
                                    game_state.clear_messages();
                                    discard_card = Some(card);
                                } else {
                                    game_state.add_message("You don't have that card!".to_string());
                                }
                            }
                            Err(err) => {
                                game_state.add_message(format!("Invalid card format: {err}"));
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
                    game_state.deck.borrow_mut().discard_pile.push_back(card);

                    game_state.add_action(
                        &current_player.name,
                        "drew and discarded the",
                        Some(card),
                    );

                    game_state
                        .display(&current_player, "Join the conversation: ")
                        .await;

                    let mut dialogue = String::new();
                    io::stdin()
                        .read_line(&mut dialogue)
                        .expect("Failed to read line");

                    game_state.push_dialogue(&current_player, dialogue.trim());
                    game_state
                        .player_dialogues
                        .borrow_mut()
                        .insert(current_player.name.clone(), dialogue);

                    // Update the player in game_state
                    game_state.players.borrow_mut()[current_idx] = current_player.clone();
                }
                Choice::Play => {
                    let (score, hand) = calculate_best_meld_from_5_card_hand(&current_player.hand);

                    let layoff_players = game_state.players.borrow().clone();

                    let (lay_offs, _winning_hand) =
                        run_layoff_round(current_idx, &hand, &layoff_players, score, &game_state)
                            .await;

                    if let Some(mut lay_offs) = lay_offs {
                        lay_offs.sort_by(|a, b| b.resulting_score.cmp(&a.resulting_score));
                        let winning_lay_off = lay_offs[0].clone();

                        let layoff_score = if winning_lay_off.cards_used == 2 {
                            0
                        } else {
                            winning_lay_off.resulting_score
                        };

                        game_state.add_action(
                            &winning_lay_off.player.name,
                            &format!(
                                "won this round with a score of {} and the hand {}",
                                &layoff_score, &winning_lay_off.resulting_hand
                            ),
                            None,
                        );

                        game_state.update_scores(&winning_lay_off.player, layoff_score as usize);
                        game_state.deal_new_round();
                    } else {
                        game_state.add_action(
                            &current_player.name,
                            &format!(
                                "won this round with a score of {} and the hand {}",
                                score, &hand
                            ),
                            None,
                        );

                        game_state.update_scores(&current_player.clone(), score as usize);
                        game_state.deal_new_round();
                    }
                }
                Choice::Retrieve => {
                    let discard_visible = game_state
                        .deck
                        .borrow_mut()
                        .discard_pile
                        .pop_back()
                        .unwrap();
                    current_player.hand.cards.push(discard_visible);

                    let mut discard_card = None;
                    while discard_card.is_none() {
                        game_state.clear_messages();
                        game_state
                            .display(&current_player, "Which card to discard?")
                            .await;

                        let mut input = String::new();
                        io::stdin()
                            .read_line(&mut input)
                            .expect("Failed to read line");

                        match Card::from_string(input.trim().to_string()) {
                            Ok(card) => {
                                if current_player.hand.cards.contains(&card) {
                                    game_state.clear_messages();
                                    discard_card = Some(card);
                                } else {
                                    game_state.add_message("You don't have that card!".to_string());
                                }
                            }
                            Err(err) => {
                                game_state.add_message(format!("Invalid card format: {err}"));
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
                    game_state.deck.borrow_mut().discard_pile.push_back(card);

                    game_state.add_action(
                        &current_player.name,
                        "retrieved the discard and discarded the",
                        discard_card,
                    );

                    game_state
                        .display(&current_player, "Join the conversation: ")
                        .await;

                    let mut dialogue = String::new();
                    io::stdin()
                        .read_line(&mut dialogue)
                        .expect("Failed to read line");

                    game_state.push_dialogue(&current_player, dialogue.trim());
                    game_state
                        .player_dialogues
                        .borrow_mut()
                        .insert(current_player.name.clone(), dialogue);

                    game_state.players.borrow_mut()[current_idx] = current_player.clone();
                }
            }

            players[current_idx] = current_player;
        }

        // Get human player from game_state, not local players
        let human_player = game_state
            .players
            .borrow()
            .iter()
            .find(|p| p.player_type.is_none())
            .cloned()
            .unwrap();

        game_state.display_updated_state(&human_player).await;

        *game_state.current_player_idx.borrow_mut() =
            (current_idx + 1) % game_state.players.borrow().len();
    }
}

fn check_for_layoff(
    player: &Player,
    played_hand: &Hand,
    score_to_beat: u64,
) -> Option<LayOffResult> {
    let mut layoff_results = Vec::new();

    for i in 0..(player.hand.cards.len().saturating_sub(1)) {
        let card_to_test = player.hand.cards[i];

        for j in 0..(played_hand.cards.len().saturating_sub(1)) {
            let mut played_cards = played_hand.cards.clone();
            played_cards.remove(j);
            played_cards.push(card_to_test);

            let resulting_hand = Hand {
                cards: played_cards,
            };

            let (score, _hand) = calculate_best_meld_from_5_card_hand(&resulting_hand);
            let layoff_result = LayOffResult {
                player: player.clone(),
                cards_laid_off: vec![card_to_test],
                resulting_hand,
                resulting_score: score,
                cards_used: 1,
            };
            layoff_results.push(layoff_result);
        }
    }

    layoff_results.retain(|r| r.resulting_score != 0);

    let mut two_card_layoff_combos = Vec::new();
    for i in 0..(player.hand.cards.len().saturating_sub(1)) {
        for j in (i + 1)..player.hand.cards.len() {
            let two_card_combo = vec![player.hand.cards[i], player.hand.cards[j]];
            two_card_layoff_combos.push(two_card_combo);
        }
    }

    let mut two_card_played_hand_combos = Vec::new();
    for i in 0..(played_hand.cards.len().saturating_sub(1)) {
        for j in (i + 1)..played_hand.cards.len() {
            let two_card_combo = vec![played_hand.cards[i], played_hand.cards[j]];
            two_card_played_hand_combos.push(two_card_combo);
        }
    }

    for two_card_played_hand_combo in two_card_played_hand_combos {
        let mut played_cards = played_hand.cards.clone();
        played_cards.retain(|c| !two_card_played_hand_combo.contains(c));
        for two_card_layoff_combo in two_card_layoff_combos.clone() {
            let mut played_cards = played_cards.clone();
            let cards_laid_off = two_card_layoff_combo.clone();
            played_cards.extend(cards_laid_off.clone());
            let resulting_hand = Hand {
                cards: played_cards.clone(),
            };
            let (score, _hand) = calculate_best_meld_from_hand(&resulting_hand);
            let layoff_result = LayOffResult {
                player: player.clone(),
                cards_laid_off: two_card_layoff_combo,
                resulting_hand,
                resulting_score: score,
                cards_used: 2,
            };
            layoff_results.push(layoff_result);
        }
    }

    let mut one_card_layoff_results: Vec<LayOffResult> = layoff_results
        .iter()
        .filter(|l| l.cards_used == 1 && l.resulting_score > 0)
        .cloned()
        .collect();

    let mut two_card_layoff_results: Vec<LayOffResult> = layoff_results
        .iter()
        .filter(|l| l.cards_used == 2 && l.resulting_score > 0)
        .cloned()
        .collect();

    let layoff_result = if !one_card_layoff_results.is_empty() {
        one_card_layoff_results.sort_by(|a, b| b.resulting_score.cmp(&a.resulting_score));
        one_card_layoff_results.first().cloned()
    } else if !two_card_layoff_results.is_empty() {
        two_card_layoff_results.sort_by(|a, b| b.resulting_score.cmp(&a.resulting_score));
        two_card_layoff_results.first().cloned()
    } else {
        None
    };

    layoff_result.filter(|result| result.resulting_score > score_to_beat)
}

fn winning_player(gs: &GameState) -> Option<Player> {
    let winning_players: Vec<Player> = gs
        .players
        .borrow()
        .iter()
        .filter(|p| p.score >= 100)
        .cloned()
        .collect();

    winning_players.first().cloned()
}

fn parse_choice(input: &str) -> Result<Choice, String> {
    match input.to_lowercase().as_str() {
        "d" | "draw" => Ok(Choice::Draw),
        "p" | "play" => Ok(Choice::Play),
        "r" | "retrieve" => Ok(Choice::Retrieve),
        _ => Err("Invalid input. Expected D (draw) or P (play) or R (retrieve).".to_string()),
    }
}
