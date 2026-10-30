//! Reference geometry: plan grid lines and stories.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GridLine {
    pub label: String,
    /// Metres along the axis the line is perpendicular to.
    pub coordinate: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Story {
    pub name: String,
    /// Metres; stories are kept sorted by elevation.
    pub elevation: f64,
}

/// A rectangular plan grid and the stories it repeats on.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Grid {
    pub x: Vec<GridLine>,
    pub y: Vec<GridLine>,
    pub stories: Vec<Story>,
}

impl Grid {
    /// Evenly spaced lines labelled 1, 2, 3… along X and A, B, C… along Y, and stories
    /// of one height above a base at 0.
    pub fn regular(bays_x: usize, bay_x: f64, bays_y: usize, bay_y: f64, stories: usize, story_height: f64) -> Self {
        Self {
            x: (0..=bays_x).map(|i| GridLine { label: (i + 1).to_string(), coordinate: i as f64 * bay_x }).collect(),
            y: (0..=bays_y).map(|i| GridLine { label: letters(i), coordinate: i as f64 * bay_y }).collect(),
            stories: std::iter::once(Story { name: "Base".to_owned(), elevation: 0.0 })
                .chain((1..=stories).map(|i| Story { name: format!("Story{i}"), elevation: i as f64 * story_height }))
                .collect(),
        }
    }

    /// Everything finite, and stories sorted by elevation.
    pub fn is_valid(&self) -> bool {
        self.x.iter().chain(&self.y).all(|l| l.coordinate.is_finite())
            && self.stories.iter().all(|s| s.elevation.is_finite())
            && self.stories.windows(2).all(|w| matches!(w, [a, b] if a.elevation < b.elevation))
    }
}

/// 0 → A, 25 → Z, 26 → AA.
fn letters(mut i: usize) -> String {
    let mut out = Vec::new();
    loop {
        out.push(b'A' + (i % 26) as u8);
        if i < 26 {
            break;
        }
        i = i / 26 - 1;
    }
    out.reverse();
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels() {
        assert_eq!(letters(0), "A");
        assert_eq!(letters(25), "Z");
        assert_eq!(letters(26), "AA");
        let g = Grid::regular(2, 8.0, 1, 6.0, 3, 3.2);
        assert_eq!(g.x.len(), 3);
        assert_eq!(g.stories.len(), 4);
        assert!(g.is_valid());
    }
}
