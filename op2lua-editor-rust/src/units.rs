//! What the editor knows about unit types: the names OP2Lua's placement takes, building footprints, which
//! structures the game clears a dozed border around, short map labels and player colours.

use std::collections::HashMap;
use std::sync::{LazyLock, RwLock};

use eframe::egui::Color32;

/// The vehicle and structure names OP2Lua's placement takes (src/Placement.cpp MapIdFromName).
pub const VEHICLES: &[&str] = &[
    "Lynx", "Panther", "Tiger", "Scorpion", "ConVec", "CargoTruck", "RoboMiner", "RoboSurveyor", "RoboDozer", "GeoCon",
    "Earthworker", "Scout", "Spider", "RepairVehicle", "EvacuationTransport",
];
pub const STRUCTURES: &[&str] = &[
    "CommandCenter", "Tokamak", "MHDGenerator", "SolarPowerArray", "GeothermalPlant", "CommonOreMine", "RareOreMine",
    "MagmaWell", "CommonOreSmelter", "RareOreSmelter", "CommonStorage", "RareStorage", "StructureFactory",
    "VehicleFactory", "ArachnidFactory", "ConsumerFactory", "Garage", "Spaceport", "BasicLab", "StandardLab",
    "AdvancedLab", "Observatory", "Residence", "ReinforcedResidence", "AdvancedResidence", "Agridome", "Nursery",
    "University", "MedicalCenter", "Forum", "RecreationFacility", "DIRT", "RobotCommand", "TradeCenter",
    "MeteorDefense", "GORF", "GuardPost", "LightTower", "Tube", "Wall", "LavaWall", "MicrobeWall",
];
/// Weapons a combat vehicle or Guard Post can carry ("" = none). EnergyCannon is the Scorpion's.
pub const WEAPONS: &[&str] = &[
    "", "Laser", "Microwave", "RailGun", "RPG", "EMP", "ESG", "Stickyfoam", "ThorsHammer", "AcidCloud", "Starflare",
    "Supernova", "EnergyCannon",
];

/// Combat vehicles the game makes only with a turret: a placement must give them a weapon.
const ARMED: &[&str] = &["Lynx", "Panther", "Tiger"];

/// Whether `unit_type` must carry a weapon.
pub fn needs_weapon(unit_type: &str) -> bool {
    ARMED.iter().any(|n| n.eq_ignore_ascii_case(unit_type))
}

/// The weapon a unit that needs one gets when none is chosen.
pub const DEFAULT_WEAPON: &str = "Laser";

/// Gives a unit that needs a weapon the default one when it has none.
pub fn ensure_weapon(unit_type: &str, weapon: &mut String) {
    if needs_weapon(unit_type) && weapon.is_empty() {
        *weapon = DEFAULT_WEAPON.to_string();
    }
}

/// Building footprints in tiles (width, height), as the X-size / Y-size columns of OPU\base\sheets\building.txt give
/// them: used until the game's own building.txt is read (`load_building_sheet`). Vehicles and anything not listed are
/// 1 x 1.
const FOOTPRINTS: &[(&str, (i32, i32))] = &[
    ("CommandCenter", (3, 2)), ("RobotCommand", (2, 2)), ("TradeCenter", (2, 2)), ("Tokamak", (2, 2)),
    ("MHDGenerator", (2, 2)), ("SolarPowerArray", (3, 2)), ("GeothermalPlant", (2, 1)), ("CommonOreMine", (2, 1)),
    ("RareOreMine", (2, 1)), ("MagmaWell", (2, 1)), ("CommonOreSmelter", (4, 3)), ("RareOreSmelter", (4, 3)),
    ("CommonStorage", (1, 2)), ("RareStorage", (1, 2)), ("StructureFactory", (4, 3)), ("VehicleFactory", (4, 3)),
    ("ArachnidFactory", (2, 2)), ("ConsumerFactory", (3, 3)), ("Garage", (3, 2)), ("Spaceport", (5, 4)),
    ("GORF", (3, 2)), ("BasicLab", (2, 2)), ("StandardLab", (3, 2)), ("AdvancedLab", (3, 3)), ("Observatory", (2, 2)),
    ("Residence", (2, 2)), ("ReinforcedResidence", (3, 2)), ("AdvancedResidence", (3, 3)), ("Agridome", (3, 2)),
    ("Nursery", (2, 2)), ("University", (2, 2)), ("MedicalCenter", (2, 2)), ("Forum", (2, 2)),
    ("RecreationFacility", (2, 2)), ("DIRT", (3, 2)), ("MeteorDefense", (2, 2)), ("GuardPost", (1, 1)),
    ("LightTower", (1, 1)), ("Tube", (1, 1)), ("Wall", (1, 1)), ("LavaWall", (1, 1)), ("MicrobeWall", (1, 1)),
];

/// Structures without tube connections (building.txt's Border column is N), used until the game's building.txt is
/// read.
const NO_TUBES: &[&str] = &[
    "Tokamak", "MHDGenerator", "GeothermalPlant", "SolarPowerArray", "LightTower", "MagmaWell", "CommonOreMine",
    "RareOreMine", "Tube", "Wall", "LavaWall", "MicrobeWall",
];

/// Tiles a player lays rather than buildings: no dozed ring, no tube connections of their own.
const LAID: &[&str] = &["Tube", "Wall", "LavaWall", "MicrobeWall"];

const ABBREVIATIONS: &[(&str, &str)] = &[
    ("CommandCenter", "CC"), ("GuardPost", "GP"), ("CargoTruck", "TRUCK"), ("ConVec", "CONVEC"), ("Lynx", "LYNX"),
    ("Tiger", "TIGER"), ("Panther", "PANTH"), ("Scorpion", "SCORP"), ("Tokamak", "TOK"), ("MHDGenerator", "MHD"),
    ("SolarPowerArray", "SOLAR"), ("StructureFactory", "SFAC"), ("VehicleFactory", "VFAC"), ("Agridome", "AGRI"),
    ("CommonOreSmelter", "SMELT"), ("CommonOreMine", "MINE"), ("Residence", "RES"), ("RoboMiner", "MINER"),
    ("RoboSurveyor", "SURV"), ("RoboDozer", "DOZER"), ("Earthworker", "EWORK"), ("RareOreSmelter", "RSMELT"),
    ("RareOreMine", "RMINE"), ("CommonStorage", "STOR"), ("RareStorage", "RSTOR"), ("GeothermalPlant", "GEO"),
    ("RecreationFacility", "REC"), ("ReinforcedResidence", "RRES"), ("AdvancedResidence", "ARES"),
    ("EvacuationTransport", "EVAC"), ("RepairVehicle", "REPAIR"), ("ArachnidFactory", "AFAC"),
    ("ConsumerFactory", "CFAC"), ("MedicalCenter", "MED"), ("MeteorDefense", "METEOR"), ("LightTower", "LIGHT"),
    ("StandardLab", "SLAB"), ("AdvancedLab", "ALAB"), ("BasicLab", "LAB"),
];

/// building.txt's code names for the structures, as the placement names them.
const CODE_NAMES: &[(&str, &str)] = &[
    ("COMMAND", "CommandCenter"), ("TOKAMAK", "Tokamak"), ("ASE", "MHDGenerator"), ("GEOTHERMAL", "GeothermalPlant"),
    ("SOLAR", "SolarPowerArray"), ("FACT_STRUCTURE", "StructureFactory"), ("TOWER_GUARD", "GuardPost"),
    ("SMELTER", "CommonOreSmelter"), ("STORAGE_ORE", "CommonStorage"), ("LAB_ADV", "AdvancedLab"),
    ("LAB_STANDARD", "StandardLab"), ("LAB", "BasicLab"), ("SMELTER_ADV", "RareOreSmelter"),
    ("STORAGE_RARE", "RareStorage"), ("FACT_VEHICLE", "VehicleFactory"), ("UNIVERSITY", "University"),
    ("NURSERY", "Nursery"), ("AGRIDOME", "Agridome"), ("ROBOT_COMMAND", "RobotCommand"), ("SPACEPORT", "Spaceport"),
    ("MED_CENTER", "MedicalCenter"), ("FACT_ANDROID", "ArachnidFactory"), ("DIRT", "DIRT"), ("GARAGE", "Garage"),
    ("GORF", "GORF"), ("RESIDENCE_E", "AdvancedResidence"), ("RESIDENCE", "Residence"),
    ("RESIDENCE_P", "ReinforcedResidence"), ("OBSERVATORY", "Observatory"), ("METEOR_DEF", "MeteorDefense"),
    ("RECREATION", "RecreationFacility"), ("FACT_LUXURY", "ConsumerFactory"), ("FORUM", "Forum"),
    ("TRADE", "TradeCenter"), ("TOWER_LIGHT", "LightTower"), ("MAGMA_WELL", "MagmaWell"), ("MINE", "CommonOreMine"),
    ("MINE_ADV", "RareOreMine"), ("TUBE", "Tube"), ("WALL", "Wall"), ("WALL_LAVA", "LavaWall"),
    ("WALL_MICROBE", "MicrobeWall"),
];

/// A structure as the game's building.txt describes it: its footprint, and its Border flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Building {
    width: i32,
    height: i32,
    /// Border Y: it connects to its colony by tubes (power plants, mines, the magma well and light towers don't).
    tubes: bool,
}

/// The game's building.txt, by lowercased placement name, once read.
static SHEET: LazyLock<RwLock<HashMap<String, Building>>> = LazyLock::new(Default::default);

/// Reads the game's building.txt (OPU\base\sheets, tab-separated: Code_Name, ..., X-size, Y-size, ..., Border) so
/// footprints and borders come from the game. Returns how many structures it named.
pub fn load_building_sheet(text: &str) -> usize {
    let mut rows = text.lines().map(|l| l.split('\t').map(str::trim).collect::<Vec<_>>());
    let Some(header) = rows.next() else { return 0 };
    let col = |name: &str| header.iter().position(|h| h.eq_ignore_ascii_case(name));
    let (Some(xs), Some(ys), Some(border)) = (col("X-size"), col("Y-size"), col("Border")) else { return 0 };
    let mut sheet = HashMap::new();
    for row in rows {
        let Some((_, name)) = CODE_NAMES.iter().find(|(code, _)| row.first() == Some(code)) else { continue };
        let num = |i: usize| row.get(i).and_then(|v| v.parse::<i32>().ok()).filter(|&v| v > 0);
        if let (Some(width), Some(height)) = (num(xs), num(ys)) {
            let tubes = row.get(border).is_some_and(|v| v.eq_ignore_ascii_case("Y"));
            sheet.insert(name.to_ascii_lowercase(), Building { width, height, tubes });
        }
    }
    let n = sheet.len();
    if n > 0 {
        *SHEET.write().unwrap_or_else(|e| e.into_inner()) = sheet;
    }
    n
}

fn sheet(unit_type: &str) -> Option<Building> {
    SHEET.read().unwrap_or_else(|e| e.into_inner()).get(&unit_type.to_ascii_lowercase()).copied()
}

/// A type's footprint in tiles (width, height); 1 x 1 for vehicles and anything not listed.
pub fn footprint(unit_type: &str) -> (i32, i32) {
    if let Some(b) = sheet(unit_type) {
        return (b.width, b.height);
    }
    FOOTPRINTS.iter().find(|(n, _)| n.eq_ignore_ascii_case(unit_type)).map_or((1, 1), |(_, s)| *s)
}

fn is_structure(unit_type: &str) -> bool {
    sheet(unit_type).is_some() || FOOTPRINTS.iter().any(|(n, _)| n.eq_ignore_ascii_case(unit_type))
}

fn is_laid(unit_type: &str) -> bool {
    LAID.iter().any(|n| n.eq_ignore_ascii_case(unit_type))
}

/// Whether the game clears a ring of dozed ground around this type: every structure (power plants and mines too),
/// not tubes and walls.
pub fn has_dozed_border(unit_type: &str) -> bool {
    is_structure(unit_type) && !is_laid(unit_type)
}

/// Whether the structure connects to its colony by tubes: building.txt's Border column (N for power plants, mines,
/// the magma well and light towers).
pub fn has_tubes(unit_type: &str) -> bool {
    if is_laid(unit_type) {
        return false;
    }
    if let Some(b) = sheet(unit_type) {
        return b.tubes;
    }
    is_structure(unit_type) && !NO_TUBES.iter().any(|n| n.eq_ignore_ascii_case(unit_type))
}

/// The two tube tiles the game lays when a structure with tube connections is placed: one tile east of its footprint
/// on its location's row, and one tile south of it on its location's column. Measured in the harness: every structure
/// of OPU 1.4.1 placed by CreateUnit from a test mission, then the cells around it read back; those
/// without tubes (power plants, light towers) get none.
pub fn dropped_tubes(unit_type: &str, x: i32, y: i32) -> Option<[(i32, i32); 2]> {
    if !has_tubes(unit_type) {
        return None;
    }
    let (_, _, x1, y1) = footprint_tiles(unit_type, x, y);
    Some([(x1 + 1, y), (x, y1 + 1)])
}

/// The status-bar tiles (inclusive) a unit at (x, y) covers. A structure's location is where its vertical and
/// horizontal tubes would cross: its centre, or right of / below it for an even size (a Tokamak's is its lower-right
/// tile). Tethys's MapObjectType::GetTileRect places the footprint the same way.
pub fn footprint_tiles(unit_type: &str, x: i32, y: i32) -> (i32, i32, i32, i32) {
    let (w, h) = footprint(unit_type);
    let (x0, y0) = (x - w / 2, y - h / 2);
    (x0, y0, x0 + w - 1, y0 + h - 1)
}

/// The short label a unit gets on the map (CC, LYNX, SFAC, ...).
pub fn abbreviation(unit_type: &str) -> String {
    ABBREVIATIONS
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(unit_type))
        .map_or_else(|| unit_type.to_ascii_uppercase(), |(_, a)| a.to_string())
}

/// A player's colour on the map (players 1-6; silver for anything else).
pub fn player_color(player: i32) -> Color32 {
    match player {
        1 => Color32::from_rgb(30, 144, 255),
        2 => Color32::from_rgb(255, 0, 0),
        3 => Color32::from_rgb(50, 205, 50),
        4 => Color32::from_rgb(255, 215, 0),
        5 => Color32::from_rgb(255, 165, 0),
        6 => Color32::from_rgb(255, 0, 255),
        _ => Color32::from_rgb(192, 192, 192),
    }
}

/// Region outline colours, by region index.
pub const REGION_COLORS: [Color32; 10] = [
    Color32::from_rgb(0, 255, 255),
    Color32::from_rgb(255, 255, 0),
    Color32::from_rgb(0, 255, 0),
    Color32::from_rgb(255, 0, 255),
    Color32::from_rgb(255, 165, 0),
    Color32::from_rgb(0, 191, 255),
    Color32::from_rgb(255, 105, 180),
    Color32::from_rgb(173, 255, 47),
    Color32::from_rgb(255, 215, 0),
    Color32::from_rgb(127, 255, 212),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn footprints_borders_and_dropped_tubes() {
        // A Command Center (3 x 2) at (42, 44) covers 41..43 x 43..44.
        assert_eq!(footprint_tiles("CommandCenter", 42, 44), (41, 43, 43, 44));
        // Every structure is dozed around; a Tokamak has no tubes, a Guard Post has (Border Y).
        assert!(has_dozed_border("Tokamak") && has_dozed_border("CommonOreMine") && has_dozed_border("CommandCenter"));
        assert!(!has_dozed_border("Tube") && !has_dozed_border("Wall") && !has_dozed_border("Lynx"));
        assert!(!has_tubes("Tokamak") && has_tubes("GuardPost") && !has_tubes("Lynx"));
        // As measured: east of the footprint on the location's row, south of it on the location's column.
        assert_eq!(dropped_tubes("CommandCenter", 5, 5), Some([(7, 5), (5, 6)]));
        assert_eq!(dropped_tubes("Spaceport", 14, 23), Some([(17, 23), (14, 25)]));
        assert_eq!(dropped_tubes("CommonStorage", 14, 14), Some([(15, 14), (14, 15)]));
        assert_eq!(dropped_tubes("GuardPost", 41, 41), Some([(42, 41), (41, 42)]));
        assert_eq!(dropped_tubes("Tokamak", 32, 5), None);
    }

    /// The game's own building.txt (OP2_DIR, else skipped) names every structure, and agrees with the built-in table.
    #[test]
    fn reads_the_games_building_sheet() {
        let Some(dir) = std::env::var_os("OP2_DIR") else { return };
        let path = std::path::PathBuf::from(dir).join("OPU").join("base").join("sheets").join("building.txt");
        let text = String::from_utf8_lossy(&std::fs::read(path).unwrap()).into_owned();
        assert_eq!(load_building_sheet(&text), CODE_NAMES.len());
        for (name, size) in FOOTPRINTS {
            assert_eq!(footprint(name), *size, "{name}");
            assert_eq!(has_tubes(name), !NO_TUBES.contains(name), "{name}");
        }
    }
}
