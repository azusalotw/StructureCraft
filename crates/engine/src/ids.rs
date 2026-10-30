//! Element ids as commands read and write them: a kind letter, the slot and its generation,
//! e.g. `N12:1` for node slot 12. The generation makes an id from before a delete fail loudly
//! instead of naming whatever reused the slot.

use structurecraft_model::{Area, Frame, Id, Material, Node, Section};

pub trait ElementRef {
    const PREFIX: char;
}

impl ElementRef for Node {
    const PREFIX: char = 'N';
}
impl ElementRef for Frame {
    const PREFIX: char = 'F';
}
impl ElementRef for Area {
    const PREFIX: char = 'A';
}
impl ElementRef for Material {
    const PREFIX: char = 'M';
}
impl ElementRef for Section {
    const PREFIX: char = 'S';
}

pub fn format_id<T: ElementRef>(id: Id<T>) -> String {
    format!("{}{}:{}", T::PREFIX, id.index(), id.generation())
}

pub fn parse_id<T: ElementRef>(s: &str) -> Option<Id<T>> {
    let rest = s.strip_prefix(T::PREFIX)?;
    let (index, generation) = rest.split_once(':')?;
    Some(Id::new(index.parse().ok()?, generation.parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_reject_other_kinds() {
        let id: Id<Node> = Id::new(12, 3);
        assert_eq!(format_id(id), "N12:3");
        assert_eq!(parse_id::<Node>("N12:3"), Some(id));
        assert_eq!(parse_id::<Frame>("N12:3"), None);
        for bad in ["", "N", "N12", "N:1", "N-1:1", "N1:x", "N99999999999:1"] {
            assert_eq!(parse_id::<Node>(bad), None, "{bad}");
        }
    }
}
