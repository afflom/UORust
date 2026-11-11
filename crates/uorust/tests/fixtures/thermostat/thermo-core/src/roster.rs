//! Rooms, their readings and the schedule that names them.

use std::collections::{BTreeMap, BTreeSet, HashMap};

/// A room and the temperature it reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Room {
    /// The room's name.
    pub name: String,
    /// The room's reading.
    pub reading: u32,
}

impl Room {
    /// True when the room reads below the floor.
    pub fn is_cold(&self, floor: u32) -> bool {
        self.reading < floor
    }

    /// The same room, read again.
    pub fn reread(&self, reading: u32) -> Self {
        Room {
            name: self.name.clone(),
            reading,
        }
    }

    /// The room's label: its name, or `unnamed`.
    pub fn label(&self) -> String {
        if self.name.is_empty() {
            return String::from("unnamed");
        }
        self.name.clone()
    }
}

/// The name of every room, in order.
pub fn names(rooms: &[Room]) -> Vec<String> {
    rooms.iter().map(|room| room.name.clone()).collect()
}

/// The rooms that read below the floor.
pub fn cold(rooms: &[Room], floor: u32) -> Vec<Room> {
    rooms.iter().filter(|room| room.is_cold(floor)).cloned().collect()
}

/// True when some room reads below the floor.
pub fn any_cold(rooms: &[Room], floor: u32) -> bool {
    rooms.iter().any(|room| room.reading < floor)
}

/// True when every reading is within the band.
pub fn all_within(readings: &[u32], low: u32, high: u32) -> bool {
    readings.iter().all(|&r| low <= r && r <= high)
}

/// The first room that reads below the floor.
pub fn first_cold(rooms: &[Room], floor: u32) -> Option<Room> {
    rooms.iter().find(|room| room.is_cold(floor)).cloned()
}

/// The highest reading, zero for no readings.
pub fn highest(readings: &[u32]) -> u32 {
    readings.iter().fold(0, |best, &r| best.max(r))
}

/// How many rooms read below the floor.
pub fn cold_count(rooms: &[Room], floor: u32) -> usize {
    rooms.iter().filter(|room| room.is_cold(floor)).count()
}

/// True when the schedule names the room.
pub fn scheduled(schedule: &[String], room: &str) -> bool {
    schedule.contains(&room.to_string())
}

/// The names joined for a display line.
pub fn roll(rooms: &[Room]) -> String {
    names(rooms).join(", ")
}

/// Each pair of a name and a reading as `name=reading` text.
pub fn pairs(entries: &[(String, i64)]) -> Vec<String> {
    entries
        .iter()
        .map(|(name, reading)| format!("{name}={reading}"))
        .collect()
}

/// The target a schedule sets for a room, read from an ordered map.
pub fn target_of(targets: &BTreeMap<String, u32>, room: &str) -> u32 {
    let Some(target) = targets.get(room) else {
        return 18;
    };
    *target
}

/// The rooms an ordered map names.
pub fn rooms_of(targets: &BTreeMap<String, u32>) -> Vec<String> {
    targets.keys().cloned().collect()
}

/// True when a hash map has a target for the room; only lookups lift on a
/// hash map, whose order is unspecified.
pub fn has_target(targets: &HashMap<String, u32>, room: &str) -> bool {
    targets.contains_key(room)
}

/// True when the room is closed.
pub fn closed(rooms: &BTreeSet<String>, room: &str) -> bool {
    rooms.contains(room)
}

/// How many characters a label has.
pub fn label_width(label: &str) -> usize {
    label.chars().count()
}

/// A reading clamped to the band, after the early exits for an empty band.
pub fn clamp(band: (u32, u32), reading: u32) -> u32 {
    let (low, high) = band;
    if high < low {
        return reading;
    }
    if reading < low {
        return low;
    }
    reading.min(high)
}

/// A blank label for a missing room.
pub fn blank() -> String {
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn room(name: &str, reading: u32) -> Room {
        Room {
            name: name.to_string(),
            reading,
        }
    }

    #[test]
    fn the_roster_reads_its_rooms() {
        let rooms = vec![room("hall", 15), room("den", 21)];
        assert_eq!(names(&rooms), vec!["hall".to_string(), "den".to_string()]);
        assert_eq!(cold(&rooms, 18), vec![room("hall", 15)]);
        assert!(any_cold(&rooms, 18));
        assert_eq!(first_cold(&rooms, 18), Some(room("hall", 15)));
        assert_eq!(cold_count(&rooms, 18), 1);
        assert_eq!(roll(&rooms), "hall, den");
        assert_eq!(room("", 1).label(), "unnamed");
        assert_eq!(room("hall", 1).reread(4).reading, 4);
    }

    #[test]
    fn the_roster_folds_and_clamps() {
        assert!(all_within(&[18, 20], 16, 22));
        assert_eq!(highest(&[3, 9, 4]), 9);
        assert_eq!(clamp((10, 20), 25), 20);
        assert_eq!(clamp((10, 20), 5), 10);
        assert_eq!(clamp((20, 10), 5), 5);
        assert_eq!(label_width("hall"), 4);
        assert!(scheduled(&["hall".to_string()], "hall"));
        assert_eq!(pairs(&[("hall".to_string(), -2)]), vec!["hall=-2".to_string()]);
        assert_eq!(blank(), "");
    }

    #[test]
    fn the_roster_reads_its_maps() {
        let targets: BTreeMap<String, u32> = [("hall".to_string(), 20)].into_iter().collect();
        assert_eq!(target_of(&targets, "hall"), 20);
        assert_eq!(target_of(&targets, "den"), 18);
        assert_eq!(rooms_of(&targets), vec!["hall".to_string()]);
        let hashed: HashMap<String, u32> = [("hall".to_string(), 20)].into_iter().collect();
        assert!(has_target(&hashed, "hall"));
        let shut: BTreeSet<String> = ["den".to_string()].into_iter().collect();
        assert!(closed(&shut, "den"));
    }
}
