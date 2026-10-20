use core::fmt;

use bitvec::order::Lsb0;
use bitvec::vec::BitVec;

use crate::Priority;

/// The two players in a parity game.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Player {
    Even,
    Odd,
}

impl Player {
    /// Constructs a player from its index. This can be used in algorithms where
    /// we have a 2-array, and 0 is Even and 1 is Odd.
    pub fn from_index(index: u8) -> Self {
        Self::try_from_index(index).unwrap_or_else(|| panic!("Invalid player index {}", index))
    }

    /// Constructs a player from its index, returning `None` for any value other
    /// than 0 (Even) or 1 (Odd). Use this when the index comes from untrusted
    /// input that should be reported as a recoverable error.
    pub fn try_from_index(index: u8) -> Option<Self> {
        match index {
            0 => Some(Player::Even),
            1 => Some(Player::Odd),
            _ => None,
        }
    }

    /// Constructs a player from a priority.
    pub fn from_priority(priority: Priority) -> Self {
        if priority.value().is_multiple_of(2) {
            Player::Even
        } else {
            Player::Odd
        }
    }

    /// Returns the index of the player, the inverse of [Self::from_index].
    pub fn to_index(&self) -> usize {
        match self {
            Player::Even => 0,
            Player::Odd => 1,
        }
    }

    /// Returns the opponent of the current player.
    pub fn opponent(&self) -> Self {
        match self {
            Player::Even => Player::Odd,
            Player::Odd => Player::Even,
        }
    }

    /// Returns the string representation of the solution for this player.
    pub fn solution(&self) -> &'static str {
        match self {
            Player::Even => "true",
            Player::Odd => "false",
        }
    }
}

impl fmt::Display for Player {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Player::Even => write!(f, "even"),
            Player::Odd => write!(f, "odd"),
        }
    }
}

/// A bit-packed vector of [`Player`] values, storing a single bit per entry.
///
/// # Details
///
/// This plays the same role for owners that [`merc_collections::ByteCompressedVec`]
/// plays for the other per-vertex and per-edge arrays of a parity game: the owner
/// array has one entry per vertex and lives as long as the game itself, so it is
/// worth storing compactly. Byte compression cannot help here, since a `Player`
/// already fits in the single byte that a `Vec<Player>` spends on it. Its actual
/// information content is one bit, which is what this stores (the [`Player::to_index`]
/// of the player), making it eight times smaller than a `Vec<Player>`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PlayerVec {
    players: BitVec<usize, Lsb0>,
}

impl PlayerVec {
    /// Creates an empty vector.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates an empty vector with room for at least `capacity` players.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            players: BitVec::with_capacity(capacity),
        }
    }

    /// Creates a vector consisting of `n` copies of the given player.
    pub fn from_elem(player: Player, n: usize) -> Self {
        Self {
            players: BitVec::repeat(Self::to_bit(player), n),
        }
    }

    /// Returns the number of players in the vector.
    pub fn len(&self) -> usize {
        self.players.len()
    }

    /// Returns true iff the vector contains no players.
    pub fn is_empty(&self) -> bool {
        self.players.is_empty()
    }

    /// Adds a player to the end of the vector.
    pub fn push(&mut self, player: Player) {
        self.players.push(Self::to_bit(player));
    }

    /// Returns the player at the given index.
    pub fn index(&self, index: usize) -> Player {
        Self::from_bit(self.players[index])
    }

    /// Sets the player at the given index.
    pub fn set(&mut self, index: usize, player: Player) {
        self.players.set(index, Self::to_bit(player));
    }

    /// Resizes the vector to the given length, filling any new entries with the given player.
    pub fn resize(&mut self, new_len: usize, player: Player) {
        self.players.resize(new_len, Self::to_bit(player));
    }

    /// Returns an iterator over the players in the vector.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = Player> + '_ {
        self.players.iter().by_vals().map(Self::from_bit)
    }

    /// Returns the bit used to represent the given player.
    fn to_bit(player: Player) -> bool {
        player.to_index() != 0
    }

    /// Returns the player represented by the given bit, the inverse of [`Self::to_bit`].
    fn from_bit(bit: bool) -> Player {
        Player::from_index(bit as u8)
    }
}

impl FromIterator<Player> for PlayerVec {
    fn from_iter<I: IntoIterator<Item = Player>>(iter: I) -> Self {
        Self {
            players: iter.into_iter().map(Self::to_bit).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use rand::RngExt;

    use merc_utilities::random_test;

    use super::Player;
    use super::PlayerVec;

    #[test]
    #[cfg_attr(miri, ignore)] // bitvec is incompatible with miri.
    fn test_player_vec_from_elem() {
        let players = PlayerVec::from_elem(Player::Odd, 3);
        assert_eq!(players.len(), 3);
        assert!(players.iter().all(|player| player == Player::Odd));
    }

    #[test]
    #[cfg_attr(miri, ignore)] // bitvec is incompatible with miri.
    fn test_player_vec_resize_keeps_existing_entries() {
        let mut players = PlayerVec::from_elem(Player::Odd, 2);
        players.resize(4, Player::Even);

        assert_eq!(
            players.iter().collect::<Vec<_>>(),
            vec![Player::Odd, Player::Odd, Player::Even, Player::Even]
        );
    }

    /// Checks the packed representation against a plain `Vec<Player>` reference.
    #[test]
    #[cfg_attr(miri, ignore)] // bitvec is incompatible with miri.
    fn test_random_player_vec() {
        random_test(100, |rng| {
            let expected: Vec<Player> = (0..rng.random_range(1..100))
                .map(|_| Player::from_index(rng.random_range(0..2)))
                .collect();

            let mut players: PlayerVec = expected.iter().cloned().collect();
            assert_eq!(players.len(), expected.len());

            for (index, player) in expected.iter().enumerate() {
                assert_eq!(players.index(index), *player);
            }
            assert_eq!(players.iter().collect::<Vec<_>>(), expected);

            // Overwriting every entry with its opponent must not disturb its neighbours.
            for (index, player) in expected.iter().enumerate() {
                players.set(index, player.opponent());
            }
            assert_eq!(
                players.iter().collect::<Vec<_>>(),
                expected.iter().map(Player::opponent).collect::<Vec<_>>()
            );
        });
    }
}
