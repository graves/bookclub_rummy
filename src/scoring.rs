use crate::card::{Card, ToU64};
use smallvec::SmallVec;
use std::collections::HashMap;

pub type CardVec = SmallVec<[Card; 6]>;

pub const MELD_FUNCTIONS: &[fn(CardVec) -> Result<u64, String>] = &[
    pair_score,
    two_pair_score,
    sequence_of_three_score,
    three_of_a_kind_score,
    straight_score,
    flush_score,
    sequence_of_four_score,
    full_set_score,
    full_house_score,
    four_of_a_kind_score,
    straight_flush_score,
    royal_flush_score,
];

/// Calculates score for having a pair in the hand.
pub fn pair_score(hand: CardVec) -> Result<u64, String> {
    for i in 0..hand.len() {
        for j in (i + 1)..hand.len() {
            if hand[i].rank == hand[j].rank {
                return Ok(2);
            }
        }
    }
    Ok(0)
}

/// Calculates score for having exactly two pairs in the hand.
pub fn two_pair_score(hand: CardVec) -> Result<u64, String> {
    let mut map = HashMap::new();
    for card in hand.iter() {
        *map.entry(card.rank).or_insert(0) += 1;
    }

    let mut pairs: Vec<usize> = map.into_values().collect::<Vec<usize>>();
    pairs.retain(|&i| i == 2);

    if pairs.len() == 2 {
        return Ok(5);
    }
    Ok(0)
}

/// Calculates score for having a sequence of three consecutive ranks.
pub fn sequence_of_three_score(hand: CardVec) -> Result<u64, String> {
    let mut values = hand
        .iter()
        .map(|c| c.rank.to_u64().unwrap())
        .collect::<Vec<u64>>();

    values.sort();
    values.dedup();

    for window in values.windows(3) {
        if window[2] == window[1] + 1 && window[1] == window[0] + 1 {
            return Ok(10);
        }
    }
    Ok(0)
}

/// Calculates score for having three cards of the same rank.
pub fn three_of_a_kind_score(hand: CardVec) -> Result<u64, String> {
    let mut map = HashMap::new();
    for card in hand.iter() {
        *map.entry(card.rank).or_insert(0) += 1;
    }

    let mut pairs: Vec<usize> = map.into_values().collect::<Vec<usize>>();
    pairs.retain(|&i| i == 3);

    if !pairs.is_empty() {
        return Ok(15);
    }
    Ok(0)
}

/// Calculates score for having a straight (5 consecutive ranks).
pub fn straight_score(hand: CardVec) -> Result<u64, String> {
    let mut values = hand
        .iter()
        .map(|c| c.rank.to_u64().unwrap())
        .collect::<Vec<u64>>();

    values.sort();

    if values.windows(2).all(|w| w[0] + 1 == w[1]) {
        return Ok(20);
    }
    Ok(0)
}

/// Calculates score for having a flush (all cards same suit).
pub fn flush_score(hand: CardVec) -> Result<u64, String> {
    let mut values = hand.iter().map(|c| c.suite).collect::<Vec<_>>();
    values.dedup();

    if values.len() == 1 {
        return Ok(25);
    }
    Ok(0)
}

/// Calculates score for having a sequence of four consecutive ranks.
pub fn sequence_of_four_score(hand: CardVec) -> Result<u64, String> {
    let mut values = hand
        .iter()
        .map(|c| c.rank.to_u64().unwrap())
        .collect::<Vec<u64>>();

    values.sort();

    let mut sequence_len = 1;

    for i in 0..(values.len() - 1) {
        if values[i] + 1 == values[i + 1] {
            sequence_len += 1;
        } else {
            sequence_len = 1;
        }
        if sequence_len == 4 {
            return Ok(30);
        }
        if sequence_len < 2 && i >= 2 {
            return Ok(0);
        }
    }
    Ok(0)
}

/// Calculates score for having a pair plus a sequence of three consecutive ranks.
pub fn full_set_score(hand: CardVec) -> Result<u64, String> {
    let mut hand_clone = hand.clone();
    let mut values = hand
        .iter()
        .map(|c| c.rank.to_u64().unwrap())
        .collect::<Vec<u64>>();

    values.sort();

    let mut pairs = values
        .windows(2)
        .filter(|vec| vec[0] == vec[1])
        .collect::<Vec<&[u64]>>();
    pairs.sort();

    if !pairs.is_empty() {
        let high_pair = pairs.last().unwrap();
        hand_clone.retain(|n| n.rank.to_u64().unwrap() != high_pair[0]);

        let mut values = hand_clone
            .iter()
            .map(|c| c.rank.to_u64().unwrap())
            .collect::<Vec<u64>>();

        values.sort();

        let mut sequence_len = 1;

        for i in 0..(values.len() - 1) {
            if values[i] + 1 == values[i + 1] {
                sequence_len += 1;
            } else {
                sequence_len = 1;
            }
            if sequence_len == 3 {
                return Ok(35);
            }
            if sequence_len < 2 && i >= 1 {
                return Ok(0);
            }
        }
    }
    Ok(0)
}

/// Calculates score for having a full house (three of a kind + pair).
pub fn full_house_score(hand: CardVec) -> Result<u64, String> {
    let mut values = hand
        .iter()
        .map(|c| c.rank.to_u64().unwrap())
        .collect::<Vec<u64>>();

    values.sort();

    let mut count = HashMap::new();
    for rank in values {
        *count.entry(rank).or_insert(0) += 1;
    }

    let mut frequencies = count.into_values().collect::<Vec<usize>>();
    frequencies.sort();

    if frequencies == vec![2, 3] {
        return Ok(40);
    }
    Ok(0)
}

/// Calculates score for having four cards of the same rank.
pub fn four_of_a_kind_score(hand: CardVec) -> Result<u64, String> {
    let mut map = HashMap::new();
    for card in hand.iter() {
        *map.entry(card.rank).or_insert(0) += 1;
    }

    let mut pairs: Vec<usize> = map.into_values().collect::<Vec<usize>>();
    pairs.retain(|&i| i == 4);

    if !pairs.is_empty() {
        return Ok(50);
    }
    Ok(0)
}

/// Calculates score for having a straight flush (straight + flush).
pub fn straight_flush_score(hand: CardVec) -> Result<u64, String> {
    if straight_score(hand.clone()).unwrap() > 0 && flush_score(hand).unwrap() > 0 {
        return Ok(80);
    }
    Ok(0)
}

/// Calculates score for having a royal flush (A, K, Q, J, 10 all same suit).
pub fn royal_flush_score(hand: CardVec) -> Result<u64, String> {
    if straight_score(hand.clone()).unwrap() > 0 && flush_score(hand.clone()).unwrap() > 0 {
        let rank_accum = hand
            .iter()
            .fold(0, |acc, card| acc + card.rank.to_u64().unwrap());
        if rank_accum == 60 {
            return Ok(100);
        }
    }
    Ok(0)
}
