use std::cmp::Ordering;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct Card {
    pub rank: Rank,
    pub suite: Suite,
    pub name: Name,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Rank {
    Two,
    Three,
    Four,
    Five,
    Six,
    Seven,
    Eight,
    Nine,
    Ten,
    Eleven,
    Twelve,
    Thirteen,
    Fourteen,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Name {
    Two,
    Three,
    Four,
    Five,
    Six,
    Seven,
    Eight,
    Nine,
    Ten,
    Jack,
    Queen,
    King,
    Ace,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Suite {
    Spades,
    Hearts,
    Clubs,
    Diamonds,
}

pub trait ToU64 {
    fn to_u64(&self) -> Result<u64, &str> {
        unimplemented!()
    }
}

pub trait ToSuite {
    fn to_suite(&self) -> Result<Suite, String> {
        unimplemented!()
    }
}

pub trait ToName {
    fn to_name(&self) -> Result<Name, String> {
        unimplemented!()
    }
}

impl Card {
    /// Creates a `Card` from a string representation.
    pub fn from_string(mut input: String) -> Result<Card, String> {
        let allowed_names = [
            "2", "3", "4", "5", "6", "7", "8", "9", "10", "J", "Q", "K", "A",
        ];
        let allowed_suites = ['s', 'h', 'c', 'd'];

        let char_count = input.chars().count();
        if !(2..=3).contains(&char_count) {
            let err_msg = "Card formatting is incorrect: {input}";
            return Err(err_msg.to_string());
        }

        let suite_char = input.pop().unwrap();
        let name_string = input;

        if !allowed_suites.contains(&suite_char) {
            let err_msg = "{suite_char} does not match any known suite!";
            return Err(err_msg.to_string());
        }

        if !allowed_names.contains(&name_string.as_str()) {
            let err_msg = "{name_string} does not match any known card name!";
            return Err(err_msg.to_string());
        }

        let name = name_string.to_name()?;
        let suite = suite_char.to_suite()?;
        let rank = name.to_rank()?;

        Ok(Card { rank, suite, name })
    }

    /// Converts a `Card` to its string representation.
    pub fn to_string(&self) -> Result<String, String> {
        let name_string = self.name.to_string()?;
        let suite_char = self.suite.to_char()?;

        Ok(format!("{name_string}{suite_char}"))
    }
}

impl Ord for Card {
    fn cmp(&self, other: &Self) -> Ordering {
        self.rank.to_u64().cmp(&other.rank.to_u64())
    }
}

impl PartialOrd for Card {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl ToSuite for char {
    #[rustfmt::skip]
    fn to_suite(&self) -> Result<Suite, String> {
        let map = HashMap::from([
            ('s', Suite::Spades),
            ('h', Suite::Hearts),
            ('c', Suite::Clubs),
            ('d', Suite::Diamonds),
        ]);

        match map.get(self) {
            Some(suite) => Ok(*suite),
            None => Err("Unknown Suite!".to_string())
        }
    }
}

impl Suite {
    pub fn to_char(&self) -> Result<char, String> {
        let map = HashMap::from([
            (Suite::Spades, '♤'),
            (Suite::Hearts, '♡'),
            (Suite::Clubs, '♧'),
            (Suite::Diamonds, '♢'),
        ]);

        match map.get(self) {
            Some(char) => Ok(*char),
            None => Err("Unknown Suite!".to_string()),
        }
    }
}

impl ToName for String {
    #[rustfmt::skip]
    fn to_name(&self) -> Result<Name, String> {
        let map = HashMap::from([
            ("2", Name::Two), ("3", Name::Three), ("4", Name::Four),
            ("5", Name::Five), ("6", Name::Six), ("7", Name::Seven),
            ("8", Name::Eight), ("9", Name::Nine), ("10", Name::Ten),
            ("J", Name::Jack), ("Q", Name::Queen), ("K", Name::King),
            ("A", Name::Ace),
        ]);

        match map.get(self.as_str()) {
            Some(name) => Ok(*name),
            None => Err("Unknown Suite!".to_string())
        }
    }
}

impl Rank {
    #[rustfmt::skip]
    pub fn to_name(&self) -> Result<Name, &str> {
        let map = HashMap::from([
            (Rank::Two, Name::Two), (Rank::Three, Name::Three),
            (Rank::Four, Name::Four), (Rank::Five, Name::Five),
            (Rank::Six, Name::Six), (Rank::Seven, Name::Seven),
            (Rank::Eight, Name::Eight), (Rank::Nine, Name::Nine),
            (Rank::Ten, Name::Ten), (Rank::Eleven, Name::Jack),
            (Rank::Twelve, Name::Queen), (Rank::Thirteen, Name::King),
            (Rank::Fourteen, Name::Ace),
        ]);

        match map.get(self) {
            Some(name) => Ok(*name),
            None => Err("Unknown Rank!")
        }
    }
}

impl Name {
    #[rustfmt::skip]
    pub fn to_rank(&self) -> Result<Rank, &str> {
        let map = HashMap::from([
            (Name::Two, Rank::Two), (Name::Three, Rank::Three),
            (Name::Four, Rank::Four), (Name::Five, Rank::Five),
            (Name::Six, Rank::Six), (Name::Seven, Rank::Seven),
            (Name::Eight, Rank::Eight), (Name::Nine, Rank::Nine),
            (Name::Ten, Rank::Ten), (Name::Jack, Rank::Eleven),
            (Name::Queen, Rank::Twelve), (Name::King, Rank::Thirteen),
            (Name::Ace, Rank::Fourteen),
        ]);

        match map.get(self) {
            Some(rank) => Ok(*rank),
            None => Err("Unknown Rank!")
        }
    }

    #[rustfmt::skip]
    pub fn to_string(&self) -> Result<String, &str> {
        let map = HashMap::from([
            (Name::Two, "2"), (Name::Three, "3"), (Name::Four, "4"),
            (Name::Five, "5"), (Name::Six, "6"), (Name::Seven, "7"),
            (Name::Eight, "8"), (Name::Nine, "9"), (Name::Ten, "10"),
            (Name::Jack, "J"), (Name::Queen, "Q"), (Name::King, "K"),
            (Name::Ace, "A"),
        ]);

        match map.get(self) {
            Some(name) => Ok((*name).to_string()),
            None => Err("Unknown Name!")
        }
    }
}

impl ToU64 for Rank {
    #[rustfmt::skip]
    fn to_u64(&self) -> Result<u64, &str> {
        let map = HashMap::from([
            (Rank::Two, 2), (Rank::Three, 3), (Rank::Four, 4),
            (Rank::Five, 5), (Rank::Six, 6), (Rank::Seven, 7),
            (Rank::Eight, 8), (Rank::Nine, 9), (Rank::Ten, 10),
            (Rank::Eleven, 11), (Rank::Twelve, 12), (Rank::Thirteen, 13),
            (Rank::Fourteen, 14),
        ]);

        match map.get(self) {
            Some(rank) => Ok(*rank),
            None => Err("Unknown Rank!")
        }
    }
}

impl ToU64 for Name {
    #[rustfmt::skip]
    fn to_u64(&self) -> Result<u64, &str> {
        let map = HashMap::from([
            (Name::Two, 2), (Name::Three, 3), (Name::Four, 4),
            (Name::Five, 5), (Name::Six, 6), (Name::Seven, 7),
            (Name::Eight, 8), (Name::Nine, 9), (Name::Ten, 10),
            (Name::Jack, 11), (Name::Queen, 12), (Name::King, 13),
            (Name::Ace, 14),
        ]);

        match map.get(self) {
            Some(rank) => Ok(*rank),
            None => Err("Unknown Rank!")
        }
    }
}

use quickcheck::{Arbitrary, Gen};

impl Arbitrary for Card {
    fn arbitrary(g: &mut Gen) -> Self {
        let all_names = [
            Name::Two,
            Name::Three,
            Name::Four,
            Name::Five,
            Name::Six,
            Name::Seven,
            Name::Eight,
            Name::Nine,
            Name::Ten,
            Name::Jack,
            Name::Queen,
            Name::King,
            Name::Ace,
        ];

        let all_suites = [Suite::Spades, Suite::Hearts, Suite::Clubs, Suite::Diamonds];

        let name = *g.choose(&all_names).unwrap();
        let rank = name.to_rank().unwrap();
        let suite = *g.choose(&all_suites).unwrap();

        Card { name, rank, suite }
    }
}
