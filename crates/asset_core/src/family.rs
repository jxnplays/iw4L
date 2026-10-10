#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum FamilyId {
    Iw4 = 0,
    Iw5 = 1,
    T5 = 2,
    T6 = 3,
    /// NX1 (Call of Duty: Future Warfare). Not a retail zone game: this tree
    /// synthesises the namespace from the Saluki asset dump, so it carries no
    /// fastfile and no string table of its own. It is a namespace so the SCAR
    /// Mod 2 files under a top-level game folder beside IW4/IW5/T5/T6, the way
    /// `CacAuthoredCategory` files weapons into classes *within* a game.
    Nx1 = 4,
}

impl FamilyId {
    pub const ALL: [Self; 5] = [
        Self::Iw4,
        Self::T5,
        Self::Iw5,
        Self::T6,
        Self::Nx1,
    ];
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Iw4 => "iw4",
            Self::Iw5 => "iw5",
            Self::T5 => "t5",
            Self::T6 => "t6",
            Self::Nx1 => "nx1",
        }
    }
    pub const fn prefix(self) -> &'static str {
        self.as_str()
    }
    pub fn parse(token: &str) -> Option<Self> {
        match token {
            "iw4" => Some(Self::Iw4),
            "iw5" => Some(Self::Iw5),
            "t5" => Some(Self::T5),
            "t6" => Some(Self::T6),
            "nx1" => Some(Self::Nx1),
            _ => None,
        }
    }
    pub fn from_prefix(prefix: &str) -> Option<Self> {
        Self::parse(prefix)
    }
    pub const fn from_zone_game(game: Self) -> Self {
        game
    }
}

mod sealed {
    pub trait Sealed {}
}
pub trait Family:
    sealed::Sealed + Copy + Clone + core::fmt::Debug + PartialEq + Eq + 'static
{
    const ID: FamilyId;
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Iw4;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Iw5;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct T5;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct T6;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Nx1;
impl sealed::Sealed for Iw4 {}
impl Family for Iw4 {
    const ID: FamilyId = FamilyId::Iw4;
}
impl sealed::Sealed for Iw5 {}
impl Family for Iw5 {
    const ID: FamilyId = FamilyId::Iw5;
}
impl sealed::Sealed for T5 {}
impl Family for T5 {
    const ID: FamilyId = FamilyId::T5;
}
impl sealed::Sealed for T6 {}
impl Family for T6 {
    const ID: FamilyId = FamilyId::T6;
}
impl sealed::Sealed for Nx1 {}
impl Family for Nx1 {
    const ID: FamilyId = FamilyId::Nx1;
}
