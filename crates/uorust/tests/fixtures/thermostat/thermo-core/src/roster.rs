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
    rooms
        .iter()
        .filter(|room| room.is_cold(floor))
        .cloned()
        .collect()
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

/// The names of the rooms below the floor, gathered by a loop.
pub fn cold_names(rooms: &[Room], floor: u32) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for room in rooms {
        if room.reading < floor {
            out.push(room.name.clone());
        }
    }
    out
}

/// A report line and the count of rooms on it, two accumulators of a loop.
pub fn report(rooms: &[Room]) -> (String, u32) {
    let mut line = String::new();
    let mut seen: u32 = 0;
    for room in rooms {
        if room.name.is_empty() {
            line.push('?');
        } else {
            line.push_str(&room.name);
        }
        line.push(';');
        seen = seen.saturating_add(1);
    }
    (line, seen)
}

/// The readings of each room, flattened over a pair pattern.
pub fn readings_of(entries: &[(String, u32)]) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::new();
    for (_, reading) in entries {
        out.push(*reading);
    }
    out
}

/// A room name read from a line such as `  hall = 20 `.
pub fn line_room(line: &str) -> Option<String> {
    let (name, _) = line.split_once('=')?;
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    Some(name.to_string())
}

/// The target a line sets, after its last `=`.
pub fn line_target(line: &str) -> Option<u32> {
    let (_, value) = line.rsplit_once("=")?;
    value.trim_start().trim_end().parse::<u32>().ok()
}

/// The rooms of a schedule, one per line, skipping comments and blanks.
pub fn schedule_rooms(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        out.push(line.to_string());
    }
    out
}

/// The rooms of a comma list, up to the first `stop`.
pub fn rooms_until_stop(list: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for item in list.split(", ") {
        if item == "stop" {
            break;
        }
        out.push(item.to_string());
    }
    out
}

/// A room tag: `room:` dropped, `!` dropped, lower case.
pub fn tag(text: &str) -> String {
    let bare = text.strip_prefix("room:").unwrap_or(text);
    let bare = bare.strip_suffix('!').unwrap_or(bare);
    bare.to_ascii_lowercase()
}

/// True when a label names a zone: it ends with `-zone` or contains `wing`.
pub fn is_zone(label: &str) -> bool {
    label.ends_with("-zone") || label.contains("wing") || label.to_ascii_uppercase() == "ZONE"
}

/// A divider of `width` dashes.
pub fn divider(width: usize) -> String {
    "-".repeat(width)
}

/// The kind of a reading's first character.
pub fn sign_kind(reading: &str) -> u8 {
    let mut kind = 0;
    for c in reading.chars() {
        kind = match c {
            '-' => 1,
            '+' => 2,
            '0'..='9' => 3,
            c if c.is_whitespace() => 4,
            _ => 5,
        };
        break;
    }
    kind
}

/// How many digits a reading has, read as bytes.
pub fn digit_count(reading: &str) -> u32 {
    let mut count: u32 = 0;
    for byte in reading.as_bytes() {
        if byte.is_ascii_digit() {
            count = count.saturating_add(1);
        }
    }
    count
}

/// The position of each room in the roster, as `index:name`.
pub fn numbered(rooms: &[Room]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (i, room) in rooms.iter().enumerate() {
        out.push(format!("{}:{}", positions()[i.min(2)], room.name));
    }
    out
}

/// The position words of the first rooms.
pub fn positions() -> Vec<String> {
    vec![
        "first".to_string(),
        "second".to_string(),
        "later".to_string(),
    ]
}

/// The first room's name, when it is the room asked for.
pub fn first_is(rooms: &[Room], name: &str) -> bool {
    rooms.first().map(|room| room.name.clone()) == Some(name.to_string())
}

/// The name before the first space of the first word list, through `?`.
pub fn first_word(text: &str) -> Option<String> {
    Some(text.split_whitespace().next()?.to_string())
}

/// The words of a label, each as its characters joined by dots.
pub fn spelled(label: &str) -> String {
    let letters: Vec<String> = label.chars().map(|c| c.to_string()).collect();
    letters.join(".")
}

/// The slots a roster needs: one per room and one spare. A length is at
/// most `isize::MAX`, so the sum fits `usize` on every target.
pub fn slots(rooms: &[Room]) -> usize {
    rooms.len() + 1
}

/// How soon a request is served, least urgent first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    /// Served last.
    Low,
    /// Served in turn.
    Normal,
    /// Served first.
    Urgent,
}

/// A request for heat: its priority, then the room it is for, which is
/// the order requests sort in.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Request {
    /// How soon it is served.
    pub priority: Priority,
    /// The room it is for.
    pub room: String,
}

/// A heating zone: one room, or a group of zones.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Zone {
    /// One room.
    Room(String),
    /// Zones heated together.
    Group(Vec<Zone>),
}

impl Zone {
    /// Whether the zone heats the room, at any depth.
    pub fn heats(&self, room: &str) -> bool {
        match self {
            Zone::Room(r) => r == room,
            Zone::Group(zones) => zones.iter().any(|z| z.heats(room)),
        }
    }
}

/// Whether every room of the zone has a name.
pub fn all_named(zone: &Zone) -> bool {
    match zone {
        Zone::Room(r) => !r.is_empty(),
        Zone::Group(zones) => zones.iter().all(|z| all_named(z)),
    }
}

/// Every room a zone heats, each once, in order.
pub fn zone_rooms(zone: &Zone) -> BTreeSet<String> {
    match zone {
        Zone::Room(r) => BTreeSet::from([r.clone()]),
        Zone::Group(zones) => {
            let mut out = BTreeSet::new();
            for z in zones {
                out.extend(zone_rooms(z));
            }
            out
        }
    }
}

/// Whether two zones heat no room in common.
pub fn apart(a: &Zone, b: &Zone) -> bool {
    let left = zone_rooms(a);
    let right = zone_rooms(b);
    left.is_disjoint(&right)
}

/// How many distinct rooms the requests are for.
pub fn requested_rooms(requests: &[Request]) -> usize {
    let mut rooms = BTreeSet::new();
    for r in requests {
        rooms.insert(r.room.clone());
    }
    rooms.len()
}

/// The rooms a zone names itself, not through a group.
pub fn own_rooms(zone: &Zone) -> Vec<String> {
    match zone {
        Zone::Group(_) => Vec::new(),
        Zone::Room(r) => vec![r.clone()],
    }
}

/// Whether every zone of a group names all its rooms, the check given by
/// its path.
pub fn all_groups_named(group: &Zone) -> bool {
    match group {
        Zone::Group(zones) => zones.as_slice().iter().all(all_named),
        Zone::Room(_) => true,
    }
}

/// Each room name, trimmed, the method given by its path.
pub fn trimmed(names: &[String]) -> Vec<String> {
    names
        .iter()
        .map(String::as_str)
        .map(str::trim)
        .map(String::from)
        .collect()
}

/// The urgent requests' rooms, read through a struct pattern.
pub fn urgent_rooms(requests: &[Request]) -> Vec<String> {
    requests
        .iter()
        .filter(|Request { priority, .. }| *priority == Priority::Urgent)
        .map(|Request { room, .. }| room.clone())
        .collect()
}

/// The width of each named band, read through a nested tuple pattern.
pub fn band_widths(bands: &[(String, (u32, u32))]) -> Vec<u32> {
    bands
        .iter()
        .map(|(_, (low, high))| high.saturating_sub(*low))
        .collect()
}

/// The readings' total, saturating, walked with `split_first`.
pub fn saturated_total(readings: &[u32]) -> u32 {
    match readings.split_first() {
        None => 0,
        Some((first, rest)) => first.saturating_add(saturated_total(rest)),
    }
}

/// Each room with its reading, paired in order, as long as the shorter.
pub fn paired(rooms: &[String], readings: &[u32]) -> Vec<(String, u32)> {
    rooms
        .iter()
        .cloned()
        .zip(readings.iter().copied())
        .collect()
}

/// Whether a request is urgent and for a named room.
pub fn urgent_and_named(request: &Request) -> bool {
    matches!(request.priority, Priority::Urgent if !request.room.is_empty())
}

/// The reading before the last, when there are two or more.
pub fn second_last(readings: &[u32]) -> Option<u32> {
    let n = readings.len();
    if n < 2 {
        return None;
    }
    readings.get(n - 2).copied()
}

/// How far a reading lies above ten, the subtraction guarded.
pub fn above_ten(reading: u32) -> u32 {
    if reading >= 10 {
        reading - 10
    } else {
        0
    }
}

/// The reading where it is warm enough.
pub fn warm(reading: u32) -> Option<u32> {
    (reading >= 18).then_some(reading)
}

/// The rooms booked for heating, in booking order and by name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bookings {
    /// The rooms in the order they were booked.
    pub order: Vec<String>,
    /// The rooms by name.
    pub by_name: BTreeSet<String>,
}

/// The bookings with one more room.
pub fn booked(bookings: &Bookings, room: &str) -> Bookings {
    let mut out = bookings.clone();
    out.order.push(room.to_string());
    out.by_name.insert(room.to_string());
    out
}

/// The larger of a room's even share and what is left, and none for no
/// rooms.
pub fn per_room(total: u32, rooms: u32) -> u32 {
    if rooms == 0 {
        return 0;
    }
    (total / rooms).max(total % rooms)
}

/// What is left of a total after an even share, unless there are no rooms.
pub fn left_over(total: u32, rooms: u32) -> Option<u32> {
    total.checked_rem(rooms)
}

/// How many times a room's name says "-up".
pub fn ups(name: &str) -> usize {
    name.matches("-up").count()
}

/// Why a roster does not check.
#[derive(Debug, PartialEq, Eq)]
pub enum RosterError {
    /// A room is listed twice.
    Twice(String),
    /// A room's schedule file is missing.
    Missing(std::path::PathBuf),
}

impl std::fmt::Display for RosterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Twice(room) => write!(f, "{room} is listed twice"),
            Self::Missing(path) => write!(f, "{} is missing", path.display()),
        }
    }
}

/// The file a room's schedule is kept in, below `root`.
pub fn schedule_file(root: &std::path::Path, room: &str) -> std::path::PathBuf {
    root.join("schedules").join(format!("{room}.toml"))
}

/// The error for a room whose schedule file is missing, below `root`.
pub fn missing_schedule(root: &str, room: &str) -> RosterError {
    RosterError::Missing(schedule_file(std::path::Path::new(root), room))
}

/// Nothing, or the first of `rooms` that `taken` already lists.
pub fn none_taken(rooms: &[String], taken: &[String]) -> Result<(), RosterError> {
    for room in rooms {
        if taken.contains(room) {
            return Err(RosterError::Twice(room.clone()));
        }
    }
    Ok(())
}

/// Nothing, or the first booking that names nobody.
pub fn all_booked(bookings: &[Option<String>]) -> Result<(), RosterError> {
    for booking in bookings {
        let Some(who) = booking else {
            return Err(RosterError::Twice("nobody".to_string()));
        };
        if who.is_empty() {
            return Err(RosterError::Twice(who.clone()));
        }
    }
    Ok(())
}

/// Both checks of a roster, in turn.
pub fn roster_checks(
    rooms: &[String],
    taken: &[String],
    bookings: &[Option<String>],
) -> Result<(), RosterError> {
    none_taken(rooms, taken)?;
    all_booked(bookings)?;
    Ok(())
}

/// A room's name as the roster files it: lowercase.
pub fn filed_name(name: &str) -> String {
    name.to_lowercase()
}

/// Whether two rooms file under one name.
pub fn same_file(a: &str, b: &str) -> bool {
    filed_name(a) == filed_name(b)
}

/// A room's name cut to `limit` characters, the last three of them an
/// ellipsis where it is cut.
pub fn cut_name(name: &str, limit: usize) -> String {
    if name.chars().count() <= limit {
        name.to_string()
    } else {
        name.chars().take(limit.saturating_sub(3)).collect::<String>() + "..."
    }
}

/// The low byte of a room count, as a one-byte register holds it.
pub fn count_byte(rooms: usize) -> u8 {
    rooms as u8
}

/// A sixteen-bit room code as an index.
pub fn code_index(code: u16) -> usize {
    code as usize
}

/// Whether a reading is in the comfortable band.
pub fn in_comfort_band(reading: u32) -> bool {
    const LOW: u32 = 18;
    const HIGH: u32 = 24;
    LOW <= reading && reading <= HIGH
}

/// Whether a booking's priority is the urgent one.
pub fn pressing(priority: Priority) -> bool {
    use Priority as P;
    matches!(priority, P::Urgent)
}

/// Whether the roster names no room, or its first room is unnamed.
pub fn starts_unnamed(rooms: &[String]) -> bool {
    if rooms.first().is_none() {
        return true;
    }
    rooms[0].is_empty()
}

/// The marked rooms' names without their mark, or `None` where a name is
/// empty or a marked name lacks its closing `!`.
pub fn marked_rooms(names: &[String]) -> Option<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for n in names {
        if n.is_empty() {
            return None;
        }
        let Some(rest) = n.strip_prefix("room-") else {
            continue;
        };
        let tail = rest.strip_suffix('!')?;
        out.push(tail.to_string());
    }
    Some(out)
}

/// The room a zone names, doubled, when it names one room.
pub fn doubled_room(zone: &Zone) -> Option<String> {
    let room = match zone {
        Zone::Room(r) => r.clone(),
        Zone::Group(_) => return None,
    };
    Some(format!("{room}{room}"))
}

/// The distinct requests, in the order they sort in.
pub fn queue(requests: &[Request]) -> BTreeSet<Request> {
    let mut out = BTreeSet::new();
    for r in requests {
        out.insert(r.clone());
    }
    out
}

/// Whether request `a` sorts before request `b`.
pub fn sorts_before(a: &Request, b: &Request) -> bool {
    a < b
}

/// Whether two zones are the same zone.
pub fn same_zone(a: &Zone, b: &Zone) -> bool {
    a == b
}

/// The set point of a room at an hour, from a table keyed by both.
pub fn set_point(
    table: &BTreeMap<(Priority, u32), u32>,
    priority: Priority,
    hour: u32,
) -> Option<u32> {
    table.get(&(priority, hour)).copied()
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
        assert_eq!(slots(&[]), 1);
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
        assert_eq!(
            pairs(&[("hall".to_string(), -2)]),
            vec!["hall=-2".to_string()]
        );
        assert_eq!(blank(), "");
    }

    #[test]
    fn the_roster_loops_over_its_rooms() {
        let rooms = vec![room("hall", 15), room("", 21)];
        assert_eq!(cold_names(&rooms, 18), vec!["hall".to_string()]);
        assert_eq!(report(&rooms), ("hall;?;".to_string(), 2));
        assert_eq!(readings_of(&[("a".to_string(), 3)]), vec![3]);
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

    #[test]
    fn the_roster_reads_text() {
        assert_eq!(line_room("  hall = 20 "), Some("hall".to_string()));
        assert_eq!(line_target("hall = 20 "), Some(20));
        assert_eq!(
            schedule_rooms("hall\n# off\n\n den \r\n"),
            vec!["hall".to_string(), "den".to_string()]
        );
        assert_eq!(
            rooms_until_stop("a, b, stop, c"),
            vec!["a".to_string(), "b".to_string()]
        );
        assert_eq!(tag("room:Hall!"), "hall");
        assert!(is_zone("east-zone"));
        assert_eq!(divider(3), "---");
        assert_eq!(sign_kind("-3"), 1);
        assert_eq!(digit_count("a12"), 2);
        assert!(first_is(&[room("hall", 1)], "hall"));
        assert_eq!(first_word("  den  hall"), Some("den".to_string()));
        assert_eq!(spelled("ab"), "a.b");
        assert_eq!(numbered(&[room("hall", 1)]), vec!["first:hall".to_string()]);
    }

    #[test]
    fn requests_sort_by_priority_then_room() {
        assert!(sorts_before(
            &Request {
                priority: Priority::Low,
                room: "hall".to_string()
            },
            &Request {
                priority: Priority::Urgent,
                room: "hall".to_string()
            }
        ));
        assert!(same_zone(
            &Zone::Room("hall".to_string()),
            &Zone::Room("hall".to_string())
        ));
        assert_eq!(set_point(&BTreeMap::new(), Priority::Low, 7), None);
        assert!(Zone::Group(vec![Zone::Room("hall".to_string())]).heats("hall"));
        assert!(apart(
            &Zone::Room("hall".to_string()),
            &Zone::Group(vec![Zone::Room("den".to_string())])
        ));
        assert_eq!(own_rooms(&Zone::Room("den".to_string())).len(), 1);
        assert_eq!(saturated_total(&[1, 2]), 3);
        assert!(!urgent_and_named(&Request {
            priority: Priority::Low,
            room: "hall".to_string()
        }));
        assert_eq!(second_last(&[4, 5, 6]), Some(5));
        assert_eq!(above_ten(12), 2);
        assert_eq!(warm(20), Some(20));
        assert_eq!(
            doubled_room(&Zone::Room("den".to_string())),
            Some("denden".to_string())
        );
        assert_eq!(doubled_room(&Zone::Group(Vec::new())), None);
        let names = |xs: &[&str]| xs.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(
            marked_rooms(&names(&["room-den!", "hall"])),
            Some(names(&["den"]))
        );
        assert_eq!(marked_rooms(&names(&["room-den"])), None);
        assert_eq!(marked_rooms(&names(&["hall", ""])), None);
        assert_eq!(ups("hall-up-up"), 2);
        assert_eq!(
            missing_schedule("/etc", "den").to_string(),
            "/etc/schedules/den.toml is missing"
        );
        assert_eq!(
            RosterError::Twice("den".to_string()).to_string(),
            "den is listed twice"
        );
        assert_eq!(
            none_taken(&["den".to_string()], &["den".to_string()]),
            Err(RosterError::Twice("den".to_string()))
        );
        assert_eq!(none_taken(&["den".to_string()], &[]), Ok(()));
        assert!(starts_unnamed(&[]));
        assert!(in_comfort_band(20));
        assert!(!in_comfort_band(30));
        assert!(pressing(Priority::Urgent));
        assert_eq!(count_byte(300), 44);
        assert_eq!(code_index(7), 7);
        assert!(same_file("Den", "den"));
        assert_eq!(
            roster_checks(&[], &[], &[None]),
            Err(RosterError::Twice("nobody".to_string()))
        );
        assert_eq!(all_booked(&[Some("ann".to_string())]), Ok(()));
        assert!(!starts_unnamed(&["den".to_string()]));
        assert_eq!(per_room(7, 2), 3);
        assert_eq!(per_room(7, 0), 0);
        assert_eq!(left_over(7, 0), None);
        assert_eq!(left_over(7, 3), Some(1));
        let none = Bookings {
            order: Vec::new(),
            by_name: BTreeSet::new(),
        };
        assert_eq!(booked(&none, "den").order, vec!["den".to_string()]);
        assert!(all_named(&Zone::Group(vec![Zone::Room(
            "hall".to_string()
        )])));
    }
}
