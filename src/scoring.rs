use crate::{
    Suite,
    card::{Card, ToU64},
};
use smallvec::SmallVec;
use std::collections::HashMap;

pub type CardVec = SmallVec<[Card; 6]>;

type MeldScoringClosure = fn(CardVec) -> Result<u64, String>;

pub const MELD_FUNCTIONS: &[MeldScoringClosure] = &[
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
    pairs.retain(|&i| i >= 2);

    if pairs.len() == 2 {
        return Ok(5);
    }
    Ok(0)
}

/// Calculates score for having a sequence of three consecutive ranks of the same suite.
pub fn sequence_of_three_score(hand: CardVec) -> Result<u64, String> {
    let mut map: HashMap<Suite, Vec<u64>> = HashMap::new();

    for card in hand {
        let entry = map.entry(card.suite).or_default();
        entry.push(card.rank.to_u64().unwrap());
    }

    let mut values = Vec::new();

    for (_suite, vals) in map.iter() {
        if vals.len() >= 3 {
            values.append(&mut vals.clone());
        }
    }

    values.dedup();
    values.sort();

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

/// Calculates score for having a sequence of four consecutive ranks o the same suite.
pub fn sequence_of_four_score(hand: CardVec) -> Result<u64, String> {
    let mut map: HashMap<Suite, Vec<u64>> = HashMap::new();

    for card in hand {
        let entry = map.entry(card.suite).or_default();
        entry.push(card.rank.to_u64().unwrap());
    }

    let mut values = Vec::new();

    for (_suite, vals) in map.iter() {
        if vals.len() >= 3 {
            values.append(&mut vals.clone());
        }
    }

    values.dedup();
    values.sort();

    if values.len() >= 4 {
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
        hand_clone.retain(|c| c.rank.to_u64().unwrap() != high_pair[0]);

        let mut values = hand_clone
            .iter()
            .map(|c| c.rank.to_u64().unwrap())
            .collect::<Vec<u64>>();

        values.sort();

        let mut sequence_len = 1;

        for i in 0..(values.len().saturating_sub(1)) {
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

/// Tests the `two_pair_score` function for 5_card hands.
#[cfg(test)]
mod tests {
    use super::*;
    use smallvec::smallvec;

    #[test]
    fn test_two_pair_score() {
        // Test case 1: Two pairs (2s, 2h, 3c, 3d) → Should return 5
        let hand: CardVec = smallvec![
            Card::from_string("2s".to_string()).unwrap(),
            Card::from_string("2h".to_string()).unwrap(),
            Card::from_string("3c".to_string()).unwrap(),
            Card::from_string("3d".to_string()).unwrap(),
            Card::from_string("4s".to_string()).unwrap(),
        ];
        let score = two_pair_score(hand).unwrap();
        assert_eq!(score, 5);

        // Test case 2: No pairs (all unique) → Should return 0
        let hand: CardVec = smallvec![
            Card::from_string("2s".to_string()).unwrap(),
            Card::from_string("3h".to_string()).unwrap(),
            Card::from_string("4c".to_string()).unwrap(),
            Card::from_string("5d".to_string()).unwrap(),
            Card::from_string("6s".to_string()).unwrap(),
        ];
        let score = two_pair_score(hand).unwrap();
        assert_eq!(score, 0);

        // Test case 3: Three of a kind (2s x3) → Should return 0
        let hand: CardVec = smallvec![
            Card::from_string("2s".to_string()).unwrap(),
            Card::from_string("2h".to_string()).unwrap(),
            Card::from_string("2c".to_string()).unwrap(),
            Card::from_string("3d".to_string()).unwrap(),
            Card::from_string("4s".to_string()).unwrap(),
        ];
        let score = two_pair_score(hand).unwrap();
        assert_eq!(score, 0);

        // Test case 4: Full house (three of a kind + pair) → Should return 5
        let hand: CardVec = smallvec![
            Card::from_string("2s".to_string()).unwrap(),
            Card::from_string("2h".to_string()).unwrap(),
            Card::from_string("2c".to_string()).unwrap(),
            Card::from_string("3d".to_string()).unwrap(),
            Card::from_string("3s".to_string()).unwrap(),
        ];
        let score = two_pair_score(hand).unwrap();
        assert_eq!(score, 5);
    }

    #[test]
    fn test_three_of_a_kind() {
        // Test case 1: Three of a kind (2s x3) → Should return 5
        let hand: CardVec = smallvec![
            Card::from_string("2s".to_string()).unwrap(),
            Card::from_string("2h".to_string()).unwrap(),
            Card::from_string("2c".to_string()).unwrap(),
            Card::from_string("3d".to_string()).unwrap(),
            Card::from_string("4s".to_string()).unwrap(),
        ];
        let score = three_of_a_kind_score(hand).unwrap();
        assert_eq!(score, 15);

        // Test case 2: Three of a kind with other cards → Should return 5
        let hand: CardVec = smallvec![
            Card::from_string("2s".to_string()).unwrap(),
            Card::from_string("3h".to_string()).unwrap(),
            Card::from_string("2c".to_string()).unwrap(),
            Card::from_string("3d".to_string()).unwrap(),
            Card::from_string("4s".to_string()).unwrap(),
        ];
        let score = three_of_a_kind_score(hand).unwrap();
        assert_eq!(score, 0);

        // Test case 3: No three of a kind → Should return 0
        let hand: CardVec = smallvec![
            Card::from_string("2s".to_string()).unwrap(),
            Card::from_string("3h".to_string()).unwrap(),
            Card::from_string("4c".to_string()).unwrap(),
            Card::from_string("5d".to_string()).unwrap(),
            Card::from_string("6s".to_string()).unwrap(),
        ];
        let score = three_of_a_kind_score(hand).unwrap();
        assert_eq!(score, 0);
    }

    #[test]
    fn test_sequence_of_three_score() {
        // Test case 1: Consecutive ranks in the same suit → Should return 10
        let hand: CardVec = smallvec![
            Card::from_string("2s".to_string()).unwrap(),
            Card::from_string("3s".to_string()).unwrap(),
            Card::from_string("4s".to_string()).unwrap(),
            Card::from_string("Kh".to_string()).unwrap(),
            Card::from_string("Ah".to_string()).unwrap(),
        ];
        let score = sequence_of_three_score(hand).unwrap();
        assert_eq!(score, 10);

        // Test case 2: Not the same suite → Should return 0
        let hand: CardVec = smallvec![
            Card::from_string("2s".to_string()).unwrap(),
            Card::from_string("3h".to_string()).unwrap(),
            Card::from_string("4s".to_string()).unwrap(),
            Card::from_string("Kh".to_string()).unwrap(),
            Card::from_string("Kh".to_string()).unwrap(),
        ];
        let score = sequence_of_three_score(hand).unwrap();
        assert_eq!(score, 0);

        // Test case 3: Three cards with two pairs (2s x2, 3h) → Should return 0
        let hand: CardVec = smallvec![
            Card::from_string("2s".to_string()).unwrap(),
            Card::from_string("2h".to_string()).unwrap(),
            Card::from_string("3s".to_string()).unwrap(),
            Card::from_string("Kh".to_string()).unwrap(),
            Card::from_string("Ah".to_string()).unwrap(),
        ];
        let score = sequence_of_three_score(hand).unwrap();
        assert_eq!(score, 0);
    }

    #[test]
    fn test_straight_score() {
        // Test case 1: Five consecutive ranks of the same suite (2s, 3s, 4s, 5s, 6s) → Should return 20
        let hand: CardVec = smallvec![
            Card::from_string("2s".to_string()).unwrap(),
            Card::from_string("3s".to_string()).unwrap(),
            Card::from_string("4s".to_string()).unwrap(),
            Card::from_string("5s".to_string()).unwrap(),
            Card::from_string("6s".to_string()).unwrap(),
        ];
        let score = straight_score(hand).unwrap();
        assert_eq!(score, 20);

        // Test case 2: Not consecutive → Should return 0
        let hand: CardVec = smallvec![
            Card::from_string("2s".to_string()).unwrap(),
            Card::from_string("3h".to_string()).unwrap(),
            Card::from_string("4s".to_string()).unwrap(),
            Card::from_string("4s".to_string()).unwrap(),
            Card::from_string("5s".to_string()).unwrap(),
        ];
        let score = straight_score(hand).unwrap();
        assert_eq!(score, 0);
    }

    #[test]
    fn test_flush_score() {
        // Test case 1: All cards same suit (2s, 3s, 4s, etc) → Should return 25
        let hand: CardVec = smallvec![
            Card::from_string("2s".to_string()).unwrap(),
            Card::from_string("3s".to_string()).unwrap(),
            Card::from_string("8s".to_string()).unwrap(),
            Card::from_string("5s".to_string()).unwrap(),
            Card::from_string("As".to_string()).unwrap(),
        ];
        let score = flush_score(hand).unwrap();
        assert_eq!(score, 25);

        // Test case 2: Mixed suits → Should return 0
        let hand: CardVec = smallvec![
            Card::from_string("2h".to_string()).unwrap(),
            Card::from_string("3h".to_string()).unwrap(),
            Card::from_string("4s".to_string()).unwrap(),
            Card::from_string("7h".to_string()).unwrap(),
            Card::from_string("8h".to_string()).unwrap(),
        ];
        let score = flush_score(hand).unwrap();
        assert_eq!(score, 0);
    }

    #[test]
    fn test_sequence_of_four_score() {
        // Test case 1: Four consecutive ranks (2s, 3s, 4s, 5s) → Should return 30
        let hand: CardVec = smallvec![
            Card::from_string("2s".to_string()).unwrap(),
            Card::from_string("3s".to_string()).unwrap(),
            Card::from_string("4s".to_string()).unwrap(),
            Card::from_string("5s".to_string()).unwrap(),
            Card::from_string("5h".to_string()).unwrap(),
        ];
        let score = sequence_of_four_score(hand).unwrap();
        assert_eq!(score, 30);

        // Test case 2: Not the same suite → Should return 0
        let hand: CardVec = smallvec![
            Card::from_string("2s".to_string()).unwrap(),
            Card::from_string("3h".to_string()).unwrap(),
            Card::from_string("4s".to_string()).unwrap(),
            Card::from_string("5h".to_string()).unwrap(),
            Card::from_string("4h".to_string()).unwrap(),
        ];
        let score = sequence_of_four_score(hand).unwrap();
        assert_eq!(score, 0);
    }

    #[test]
    fn test_full_set_score() {
        // Test case 1: Full set (pair + sequence of three) → Should return 35
        let hand: CardVec = smallvec![
            Card::from_string("2s".to_string()).unwrap(),
            Card::from_string("2h".to_string()).unwrap(),
            Card::from_string("3s".to_string()).unwrap(),
            Card::from_string("4d".to_string()).unwrap(),
            Card::from_string("5h".to_string()).unwrap(),
        ];
        let score = full_set_score(hand).unwrap();
        assert_eq!(score, 35);

        // Test case 2: Only pair → Should return 0
        let hand: CardVec = smallvec![
            Card::from_string("2s".to_string()).unwrap(),
            Card::from_string("2h".to_string()).unwrap(),
            Card::from_string("3s".to_string()).unwrap(),
            Card::from_string("3h".to_string()).unwrap(),
            Card::from_string("4h".to_string()).unwrap(),
        ];
        let score = full_set_score(hand).unwrap();
        assert_eq!(score, 0);
    }

    #[test]
    fn test_full_house_score() {
        // Test case 1: Full house (three of a kind + pair) → Should return 40
        let hand: CardVec = smallvec![
            Card::from_string("2s".to_string()).unwrap(),
            Card::from_string("2h".to_string()).unwrap(),
            Card::from_string("2c".to_string()).unwrap(),
            Card::from_string("3s".to_string()).unwrap(),
            Card::from_string("3h".to_string()).unwrap(),
        ];
        let score = full_house_score(hand).unwrap();
        assert_eq!(score, 40);

        // Test case 2: Only three of a kind → Should return 0
        let hand: CardVec = smallvec![
            Card::from_string("2s".to_string()).unwrap(),
            Card::from_string("2h".to_string()).unwrap(),
            Card::from_string("2c".to_string()).unwrap(),
            Card::from_string("3s".to_string()).unwrap(),
            Card::from_string("4s".to_string()).unwrap(),
        ];
        let score = full_house_score(hand).unwrap();
        assert_eq!(score, 0);
    }

    #[test]
    fn test_four_of_a_kind_score() {
        // Test case 1: Four of a kind (2s x4) → Should return 50
        let hand: CardVec = smallvec![
            Card::from_string("2s".to_string()).unwrap(),
            Card::from_string("2h".to_string()).unwrap(),
            Card::from_string("2c".to_string()).unwrap(),
            Card::from_string("2d".to_string()).unwrap(),
            Card::from_string("3d".to_string()).unwrap(),
        ];
        let score = four_of_a_kind_score(hand).unwrap();
        assert_eq!(score, 50);

        // Test case 2: Three of a kind → Should return 0
        let hand: CardVec = smallvec![
            Card::from_string("2s".to_string()).unwrap(),
            Card::from_string("2h".to_string()).unwrap(),
            Card::from_string("2h".to_string()).unwrap(),
            Card::from_string("4c".to_string()).unwrap(),
            Card::from_string("4d".to_string()).unwrap(),
        ];
        let score = four_of_a_kind_score(hand).unwrap();
        assert_eq!(score, 0);
    }

    #[test]
    fn test_straight_flush_score() {
        // Test case 1: Straight flush (2s, 3s, 4s) → Should return 80
        let hand: CardVec = smallvec![
            Card::from_string("2s".to_string()).unwrap(),
            Card::from_string("3s".to_string()).unwrap(),
            Card::from_string("4s".to_string()).unwrap(),
            Card::from_string("5s".to_string()).unwrap(),
            Card::from_string("6s".to_string()).unwrap(),
        ];
        let score = straight_flush_score(hand).unwrap();
        assert_eq!(score, 80);

        // Test case 2: Flush but different suites → Should return 0
        let hand: CardVec = smallvec![
            Card::from_string("2s".to_string()).unwrap(),
            Card::from_string("3h".to_string()).unwrap(),
            Card::from_string("4s".to_string()).unwrap(),
            Card::from_string("5h".to_string()).unwrap(),
            Card::from_string("6s".to_string()).unwrap(),
        ];
        let score = straight_flush_score(hand).unwrap();
        assert_eq!(score, 0);
    }

    #[test]
    fn test_royal_flush_score() {
        // Test case 1: Royal flush (A, K, Q, J, 10) → Should return 100
        let hand: CardVec = smallvec![
            Card::from_string("As".to_string()).unwrap(),
            Card::from_string("Ks".to_string()).unwrap(),
            Card::from_string("Qs".to_string()).unwrap(),
            Card::from_string("Js".to_string()).unwrap(),
            Card::from_string("10s".to_string()).unwrap(),
        ];
        let score = royal_flush_score(hand).unwrap();
        assert_eq!(score, 100);

        // Test case 2: Not the same suite → Should return 0
        let hand: CardVec = smallvec![
            Card::from_string("As".to_string()).unwrap(),
            Card::from_string("Ks".to_string()).unwrap(),
            Card::from_string("Qh".to_string()).unwrap(),
            Card::from_string("Js".to_string()).unwrap(),
            Card::from_string("10s".to_string()).unwrap(),
        ];
        let score = royal_flush_score(hand).unwrap();
        assert_eq!(score, 0);

        // Test case 3: Not the highest ranks → Should return 0
        let hand: CardVec = smallvec![
            Card::from_string("As".to_string()).unwrap(),
            Card::from_string("Ks".to_string()).unwrap(),
            Card::from_string("Qh".to_string()).unwrap(),
            Card::from_string("Js".to_string()).unwrap(),
            Card::from_string("10s".to_string()).unwrap(),
        ];
        let score = royal_flush_score(hand).unwrap();
        assert_eq!(score, 0);
    }
}
