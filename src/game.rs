use crate::card::Card;
use rand::prelude::SliceRandom;
use rand::rng;
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};

#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Deck<'a> {
    pub draw_pile: &'a mut VecDeque<Card>,
    pub discard_pile: &'a mut VecDeque<Card>,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Hand {
    pub cards: Vec<Card>,
}

#[derive(Clone, Debug)]
pub struct Player {
    pub name: String,
    pub description: String,
    pub player_type: Option<PlayerType>,
    pub hand: Hand,
    pub actions: VecDeque<ActionHistory>,
    pub dialogue: VecDeque<String>,
    pub score: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PlayerType {
    Conservative,
    Aggressive,
    Balanced,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PlayAction {
    Draw,     // Draw one card (discard one card)
    Play,     // Play the current hand
    Retrieve, // Draw from the discard pil
}

#[derive(Clone, Debug)]
pub struct AutoPlayDecision {
    pub action: PlayAction,
    pub confidence: f64,
    pub expected_score: f64,
    pub card_to_discard: Option<Card>, // Which card to discard if drawing
}

#[derive(Clone, Debug)]
pub struct ActionHistory {
    pub choice: Choice,
    pub card_to_discard: Option<Card>, // Which card to discard if drawing
}

#[derive(Clone, Debug)]
pub enum Choice {
    Draw,
    Play,
    Retrieve,
}

impl PartialEq for Player {
    fn eq(&self, other: &Self) -> bool {
        // Only compare the "name" field
        self.name == other.name
    }
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

type PlayersAndPiles = (Vec<Player>, VecDeque<Card>, VecDeque<Card>);

/// Deals 5 cards to each player from the deck.
pub fn deal_cards<'a>(
    mut players: Vec<Player>,
    deck: RefCell<&mut Deck<'a>>,
) -> Result<PlayersAndPiles, String> {
    for _ in 0..5 {
        for player in players.iter_mut() {
            let card = deck
                .borrow_mut()
                .draw_pile
                .pop_back()
                .ok_or("Deck is empty")?;
            player.hand.cards.push(card);
        }
    }

    let draw_pile = deck
        .borrow_mut()
        .draw_pile
        .iter()
        .copied()
        .collect::<VecDeque<Card>>();
    let discard_pile = deck
        .borrow_mut()
        .discard_pile
        .iter()
        .copied()
        .collect::<VecDeque<Card>>();

    Ok((players, draw_pile, discard_pile))
}

/// Calculates the best possible meld score from a 6-card hand by trying all 5-card combinations
pub fn calculate_best_meld_from_hand(hand: &Hand) -> (u64, Hand) {
    use crate::scoring::{CardVec, MELD_FUNCTIONS};
    let mut score_to_hand = HashMap::new();

    // Try all possible 5-card combinations from the 6-card hand
    for skip_idx in 0..hand.cards.len() {
        let mut five_card_hand = CardVec::new();
        for (i, &card) in hand.cards.iter().enumerate() {
            if i != skip_idx {
                five_card_hand.push(card);
            }
        }

        if five_card_hand.len() == 5 {
            for meld_fn in MELD_FUNCTIONS {
                let score = meld_fn(five_card_hand.clone());
                score_to_hand
                    .entry(score)
                    .or_insert_with(|| five_card_hand.clone());
            }
        }
    }

    if let Some((best_score, high_hand)) = score_to_hand.iter().max_by_key(|(score, _)| *score) {
        match best_score {
            Ok(score) => {
                let mut card_vec = Vec::new();
                for card in high_hand {
                    card_vec.push(*card);
                }
                let hand = Hand { cards: card_vec };

                return (*score, hand);
            }
            _ => return (0, hand.clone()),
        }
    }

    (0, hand.clone())
}

pub fn calculate_best_meld_from_5_card_hand(hand: &Hand) -> (u64, Hand) {
    use crate::scoring::{CardVec, MELD_FUNCTIONS};

    let mut best_score = 0;
    let mut five_card_hand = CardVec::new();

    for card in &hand.cards {
        five_card_hand.push(*card);
    }

    for meld_fn in MELD_FUNCTIONS {
        let score = meld_fn(five_card_hand.clone()).unwrap();
        if score > best_score {
            best_score = score;
        }
    }

    (best_score, hand.clone())
}

impl<'a> Deck<'a> {
    pub fn reshuffle_deck(&mut self) -> Result<(), String> {
        let mut deck: Vec<Card> = (*self.discard_pile).clone().into();

        deck.shuffle(&mut rng());

        *self.draw_pile = VecDeque::from(deck);
        *self.discard_pile = VecDeque::new();

        Ok(())
    }
}
