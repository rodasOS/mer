use serde::{Deserialize, Serialize};

/// A color the user can give to a space or a tab. The UI maps each one to a concrete tone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Accent {
    Red,
    Orange,
    Amber,
    Lime,
    Green,
    Teal,
    Cyan,
    Blue,
    Violet,
    Pink,
}

impl Accent {
    pub const ALL: [Self; 10] = [
        Self::Red,
        Self::Orange,
        Self::Amber,
        Self::Lime,
        Self::Green,
        Self::Teal,
        Self::Cyan,
        Self::Blue,
        Self::Violet,
        Self::Pink,
    ];

    /// Stable name, used for storage.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Red => "red",
            Self::Orange => "orange",
            Self::Amber => "amber",
            Self::Lime => "lime",
            Self::Green => "green",
            Self::Teal => "teal",
            Self::Cyan => "cyan",
            Self::Blue => "blue",
            Self::Violet => "violet",
            Self::Pink => "pink",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|accent| accent.name() == name)
    }

    /// The color handed to the `n`-th space created, so neighbours look different.
    pub fn rotation(n: usize) -> Self {
        // Skips neighbours on the color wheel: 7 is coprime with 10.
        Self::ALL[(n * 7) % Self::ALL.len()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for accent in Accent::ALL {
            assert_eq!(Accent::from_name(accent.name()), Some(accent));
        }
        assert_eq!(Accent::from_name("plaid"), None);
    }

    #[test]
    fn rotation_does_not_repeat_within_a_cycle() {
        let firsts: std::collections::HashSet<_> = (0..10).map(Accent::rotation).collect();
        assert_eq!(firsts.len(), 10);
        assert_ne!(Accent::rotation(0), Accent::rotation(1));
    }

    #[test]
    fn serde_uses_snake_case() {
        assert_eq!(serde_json::to_string(&Accent::Violet).unwrap(), "\"violet\"");
    }
}
