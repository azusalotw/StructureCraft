use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LengthUnit {
    Millimetre,
    Centimetre,
    #[default]
    Metre,
    Inch,
    Foot,
}

impl LengthUnit {
    /// Metres per unit.
    pub fn in_metres(self) -> f64 {
        match self {
            Self::Millimetre => 1e-3,
            Self::Centimetre => 1e-2,
            Self::Metre => 1.0,
            Self::Inch => 0.0254,
            Self::Foot => 0.3048,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ForceUnit {
    Newton,
    #[default]
    Kilonewton,
    KilogramForce,
    TonneForce,
    Kip,
}

impl ForceUnit {
    /// Newtons per unit.
    pub fn in_newtons(self) -> f64 {
        match self {
            Self::Newton => 1.0,
            Self::Kilonewton => 1e3,
            Self::KilogramForce => 9.806_65,
            Self::TonneForce => 9_806.65,
            Self::Kip => 4_448.221_615_260_5,
        }
    }
}

/// Display units: what the user types and reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Units {
    pub length: LengthUnit,
    pub force: ForceUnit,
}
