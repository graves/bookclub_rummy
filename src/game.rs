use crate::card::Card;
use rand::prelude::SliceRandom;
use rand::rng;
use std::collections::VecDeque;

#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Deck<'a> {
    pub draw_pile: &'a mut VecDeque<Card>,
    pub discard_pile: &'a mut VecDeque<Card>,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Hand {
    pub cards: Vec<Card>,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Player {
    pub name: String,
    pub hand: Hand,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PlayerType {
    Conservative,
    Aggressive,
    Balanced,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PlayAction {
    Draw, // Draw one card (discard one card)
    Play, // Play the current hand
}

#[derive(Clone, Debug)]
pub struct AutoPlayDecision {
    pub action: PlayAction,
    pub confidence: f64,
    pub expected_score: f64,
    pub card_to_discard: Option<Card>, // Which card to discard if drawing
}

/// Creates and shuffles a standard 52-card deck.
pub fn shuffle_deck() -> Result<VecDeque<Card>, String> {
    use crate::card::{Suite, ToName};

    let mut deck: Vec<Card> = [Suite::Spades, Suite::Hearts, Suite::Diamonds, Suite::Clubs]
        .iter()
        .flat_map(|suite: &Suite| {
            let cards = [
                "2", "3", "4", "5", "6", "7", "8", "9", "10", "J", "Q", "K", "A",
            ]
            .map(|name_string| {
                let name = (*name_string).to_string().to_name().unwrap();
                let rank = name.to_rank().unwrap();

                Card {
                    name,
                    rank,
                    suite: *suite,
                }
            });

            cards.to_vec()
        })
        .collect::<Vec<Card>>();

    deck.shuffle(&mut rng());

    Ok(VecDeque::from(deck))
}

/// Deals 5 cards to each player from the deck.
pub fn deal_cards<'a>(
    mut players: Vec<Player>,
    deck: &'a mut Deck<'a>,
) -> Result<(Vec<Player>, &'a mut Deck<'a>), String> {
    for _ in 0..5 {
        for player in players.iter_mut() {
            let card = deck.draw_pile.pop_back().ok_or("Deck is empty")?;
            player.hand.cards.push(card);
        }
    }

    Ok((players, deck))
}

/// Calculates the best possible meld score from a 6-card hand by trying all 5-card combinations
pub fn calculate_best_meld_from_hand(hand: &Hand) -> u64 {
    use crate::scoring::{CardVec, MELD_FUNCTIONS};

    if hand.cards.len() < 5 {
        return 0;
    }

    let mut best_score = 0;

    // Try all possible 5-card combinations from the 6-card hand
    for skip_idx in 0..hand.cards.len() {
        let mut five_card_hand = CardVec::new();
        for (i, &card) in hand.cards.iter().enumerate() {
            if i != skip_idx {
                five_card_hand.push(card);
            }
        }

        if five_card_hand.len() == 5 {
            let hand_score = MELD_FUNCTIONS
                .iter()
                .filter_map(|&meld_fn| meld_fn(five_card_hand.clone()).ok())
                .max()
                .unwrap_or(0);

            best_score = best_score.max(hand_score);
        }
    }

    best_score
}
