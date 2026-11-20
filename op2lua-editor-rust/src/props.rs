//! A mission's properties (the top of its placement.lua: name, map, tech, type, max_tech, players_count), kept in
//! step with its DLL, and New Mission's files.

use std::path::{Path, PathBuf};

use op2_formats::placement::{replace_scalar_int, replace_scalar_string, scalar_int, scalar_string};

use crate::dll;

#[derive(Debug, Clone, PartialEq, Eq)]
/// The mission's properties as placement.lua writes them: `type` is a name such as Colony or
/// MultiLastOneStanding (or a campaign mission's number), `max_tech` the highest tech level, `players` the
/// player count the DLL declares.
pub struct Props {
    pub name: String,
    pub map: String,
    pub tech: String,
    pub mission_type: String,
    pub max_tech: i32,
    pub players: i32,
}

impl Default for Props {
    fn default() -> Self {
        Self {
            name: String::new(),
            map: "eden01.map".into(),
            tech: "MULTITEK.TXT".into(),
            mission_type: "Colony".into(),
            max_tech: 12,
            players: 2,
        }
    }
}

impl Props {
    /// The properties a placement's text declares (defaults for the ones it doesn't).
    pub fn read(text: &str) -> Self {
        let d = Self::default();
        let s = |k: &str, def: &str| {
            scalar_string(text, k).filter(|v| !v.is_empty()).unwrap_or_else(|| def.to_string())
        };
        Self {
            name: scalar_string(text, "name").unwrap_or_default(),
            map: scalar_string(text, "map").unwrap_or_default(),
            tech: s("tech", &d.tech),
            mission_type: s("type", &d.mission_type),
            max_tech: scalar_int(text, "max_tech").unwrap_or(d.max_tech),
            players: scalar_int(text, "players_count").unwrap_or(d.players),
        }
    }

    /// `text` with these properties written over the ones it declares.
    pub fn write_into(&self, text: &str) -> String {
        let mut t = replace_scalar_string(text, "name", &self.name);
        t = replace_scalar_string(&t, "map", &self.map);
        t = replace_scalar_string(&t, "tech", &self.tech);
        t = replace_scalar_string(&t, "type", &self.mission_type);
        t = replace_scalar_int(&t, "max_tech", self.max_tech);
        replace_scalar_int(&t, "players_count", self.players)
    }

    /// Why these properties can't be used, if they can't.
    pub fn problem(&self) -> Option<String> {
        if self.name.trim().is_empty() {
            return Some("The name can't be empty.".into());
        }
        if !self.name.is_ascii() || !self.map.is_ascii() {
            return Some("The name and map are stored in the mission DLL as plain ASCII.".into());
        }
        if self.map.trim().is_empty() {
            return Some("Choose a map.".into());
        }
        if dll::mission_type_value(&self.mission_type).is_none() {
            return Some(format!(
                "The type must be one of {}, or a campaign mission's number (1 to 12).",
                dll::MISSION_TYPES.join(", ")
            ));
        }
        None
    }
}

/// The mission DLL beside a placement file: `c<Base>.dll`, else `<Base>.dll`.
pub fn mission_dll(placement: &Path) -> Option<PathBuf> {
    let base = crate::mission::base_name(placement);
    let dir = placement.parent()?;
    [dir.join(format!("c{base}.dll")), dir.join(format!("{base}.dll"))].into_iter().find(|p| p.is_file())
}

/// Brings a mission DLL in step with changed properties: its LevelDesc and FileDescription (name), MapName and
/// DescBlock (type, players, max tech). The previous DLL is kept as `.bak`. Returns what was done.
pub fn patch_mission_dll(path: &Path, old: &Props, new: &Props) -> Result<String, String> {
    let mut bytes = std::fs::read(path).map_err(|e| format!("Can't read {}: {e}", path.display()))?;
    let (mut patched, mut failed) = (0, Vec::new());
    if new.name != old.name {
        if dll::patch_ascii_field(&mut bytes, &old.name, &new.name) {
            patched += 1;
        } else {
            failed.push("name");
        }
        dll::patch_wide_field(&mut bytes, &old.name, &new.name); // FileDescription, best effort
    }
    if new.map != old.map {
        if dll::patch_ascii_field(&mut bytes, &old.map, &new.map) {
            patched += 1;
        } else {
            failed.push("map");
        }
    }
    if dll::mission_type_value(&new.mission_type) != dll::mission_type_value(&old.mission_type)
        || new.players != old.players
        || new.max_tech != old.max_tech
    {
        let t = dll::mission_type_value(&new.mission_type).unwrap_or(-1);
        if dll::patch_desc_block(&mut bytes, t, new.players, new.max_tech) {
            patched += 1;
        } else {
            failed.push("DescBlock");
        }
    }
    let file = path.file_name().unwrap_or_default().to_string_lossy();
    if patched > 0 {
        let bak = PathBuf::from(format!("{}.bak", path.display()));
        std::fs::copy(path, &bak).map_err(|e| format!("Can't back up {}: {e}", path.display()))?;
        std::fs::write(path, &bytes).map_err(|e| format!("Can't write {}: {e}", path.display()))?;
    }
    Ok(match (patched, failed.is_empty()) {
        (0, true) => format!("{file} needed no change."),
        (n, true) => format!("{file}: patched {n} field(s)."),
        (n, false) => format!("{file}: patched {n} field(s); couldn't find {} (left as it was).", failed.join(", ")),
    })
}

/// A mission's base name: its name's letters and digits (as op2lua-newmission makes it).
pub fn base_name_from(name: &str) -> String {
    name.chars().filter(|c| c.is_ascii_alphanumeric()).collect()
}

/// The three files New Mission writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewFiles {
    /// The scripts' base name (`<base>.lua`, `<base>.placement.lua`) and the three paths.
    pub base: String,
    pub dll: PathBuf,
    pub script: PathBuf,
    pub placement: PathBuf,
}

/// Where New Mission puts a mission named by `props` in `dir`. Outpost 2 lists a DLL whose name starts with c under
/// Colony Games, and OP2LuaCore finds cMyMission.dll's scripts as MyMission.* (it drops a c before a capital), so a
/// base without a leading capital keeps the c on the scripts too. Multiplayer missions have no c.
pub fn new_files(dir: &Path, props: &Props) -> Option<NewFiles> {
    let mut base = base_name_from(&props.name);
    let first = base.chars().next()?;
    let colony = !dll::is_multiplayer_type(&props.mission_type);
    let dll_base = if colony { format!("c{base}") } else { base.clone() };
    if colony && !first.is_ascii_uppercase() {
        base = dll_base.clone();
    }
    Some(NewFiles {
        dll: dir.join(format!("{dll_base}.dll")),
        script: dir.join(format!("{base}.lua")),
        placement: dir.join(format!("{base}.placement.lua")),
        base,
    })
}

/// Writes a new mission: the stub DLL patched with its name, map, type, players and max tech, and starter scripts.
pub fn create(files: &NewFiles, props: &Props) -> Result<(), String> {
    let mut bytes = dll::STUB.to_vec();
    if !dll::patch_ascii_field(&mut bytes, dll::STUB_NAME, &props.name) {
        return Err("The bundled stub DLL lacks the name placeholder.".into());
    }
    dll::patch_ascii_field(&mut bytes, dll::STUB_MAP, &props.map);
    dll::patch_wide_field(&mut bytes, dll::STUB_DESC, &props.name);
    let t = dll::mission_type_value(&props.mission_type).ok_or("Unknown mission type.")?;
    if !dll::patch_desc_block(&mut bytes, t, props.players, props.max_tech) {
        return Err("The bundled stub DLL has no DescBlock export.".into());
    }
    if let Some(dir) = files.dll.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("Can't create {}: {e}", dir.display()))?;
    }
    let write = |p: &Path, data: &[u8]| std::fs::write(p, data).map_err(|e| format!("Can't write {}: {e}", p.display()));
    write(&files.dll, &bytes)?;
    write(&files.placement, placement_skeleton(&files.base, props).as_bytes())?;
    write(&files.script, script_skeleton(&files.base, props).as_bytes())?;
    Ok(())
}

/// The starter placement.lua: a small two-player fight, or for a multiplayer type a Command Center per player and no
/// players table (the lobby sets each player's colony and colour).
pub fn placement_skeleton(base: &str, p: &Props) -> String {
    let q = |s: &str| format!("\"{s}\"");
    let mut s = format!(
        "-- {base}.placement.lua  -  {}  -  starting layout.\n\
         -- Coordinates are the ones shown on the in-game status bar (hover a tile to read them).\n\
         return {{\n  name = {}, map = {}, tech = {}, type = {},\n  max_tech = {}, players_count = {},\n\n",
        p.name,
        q(&p.name),
        q(&p.map),
        q(&p.tech),
        q(&p.mission_type),
        p.max_tech,
        p.players
    );
    if dll::is_multiplayer_type(&p.mission_type) {
        s += "  -- No players table: in a multiplayer game the lobby has already set each player's colony and colour.\n\n";
        s += "  units = {\n";
        s += &format!("    -- What every game starts with (player = 1..{}). Bases that depend on the lobby's\n", p.players);
        s += "    -- settings (game.initial_units()) are made in on_init instead.\n";
        for k in 1..=p.players {
            s += &format!("    {{ type = \"CommandCenter\", player = {k}, at = {{ {}, 20 }} }},\n", 20 + 12 * (k - 1));
        }
        s += "  },\n\n  beacons = {}, walls = {},\n  regions = {},\n  markers = {},\n}\n";
        return s;
    }
    s += r#"  players = {
    [1] = { colony = "Eden", human = true, color = "Blue",
            resources = { common_ore = 3000, food = 3000, kids = 8, workers = 12, scientists = 6, tech_level = 9 },
            center_view = { 30, 55 } },
    [2] = { colony = "Plymouth", human = false, color = "Red",
            resources = { common_ore = 4000, food = 4000, kids = 15, workers = 20, scientists = 10, tech_level = 12 } },
  },

  units = {
    -- Your starting force (player 1).
    { type = "Lynx",          player = 1, at = { 30, 54 }, weapon = "Laser" },
    { type = "Lynx",          player = 1, at = { 31, 54 }, weapon = "RPG" },
    { type = "CommandCenter", player = 1, at = { 28, 56 } },
    { type = "Tokamak",       player = 1, at = { 31, 57 } },

    -- The enemy you fight (player 2). A powered Guard Post will fire at you.
    { type = "CommandCenter", player = 2, at = { 58, 44 } },
    { type = "Tokamak",       player = 2, at = { 60, 44 } },
    { type = "GuardPost",     player = 2, at = { 58, 46 }, weapon = "RPG" },
  },

  beacons = {}, walls = {},
  regions = { enemy_base = { 54, 40, 64, 48 } },   -- a rectangle: { x1, y1, x2, y2 }
  markers = {},
}
"#;
    s
}

/// The starter `<Base>.lua`: win or lose on the Command Centers, or for a multiplayer type the lobby's settings and
/// the type's victory.
pub fn script_skeleton(base: &str, p: &Props) -> String {
    let mut s = format!(
        "-- {base}.lua  -  {}  -  mission logic.\n-- See docs/API.md for the full API. Edit freely; this is your mission.\n\n",
        p.name
    );
    if dll::is_multiplayer_type(&p.mission_type) {
        s += "function on_init()\n";
        s += "  -- The lobby's settings: game.initial_units(), game.uses_day_night(), game.uses_morale(),\n";
        s += "  -- game.disasters_enabled(), and each player's .difficulty (the resource level).\n";
        if dll::mission_type_value(&p.mission_type) == Some(-8) {
            s += "  mission.last_one_standing()   -- the game's own Last One Standing victory, per player\n";
        } else {
            s += "  -- Win: this type's victory with trigger.* and mission.victory (docs/API.md, \"Multiplayer missions\").\n";
        }
        s += "end\n";
        return s;
    }
    s += &format!("function on_init()\n  game.message(\"{}: destroy the enemy Command Center!\")\n", p.name.replace('"', "\\\""));
    s += r#"  game.sound("NewMissionObjective")
  game.morale_steady()   -- keep morale stable so it's about the fight, not colony micro

  -- WIN: the enemy has no Command Center left.
  when(function() return not players[2]:owns_any("CommandCenter") end, function()
    game.message("Enemy base destroyed - mission accomplished!")
    game.sound("StructureDestroyed")
    mission.win()
  end)

  -- LOSE: your colony's Command Center is gone.
  when(function() return not players[1]:owns_any("CommandCenter") end, function()
    game.message("Your Command Center is destroyed. Mission failed.")
    mission.lose()
  end)
end
"#;
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use op2_formats::mission::{MissionDll, MissionType};

    #[test]
    fn colony_names_get_the_c() {
        let p = Props { name: "Iron Front!".into(), ..Default::default() };
        let f = new_files(Path::new("LUA"), &p).unwrap();
        assert_eq!((f.dll.file_name().unwrap().to_str().unwrap(), f.base.as_str()), ("cIronFront.dll", "IronFront"));
        let p = Props { name: "rescue".into(), ..Default::default() };
        let f = new_files(Path::new("LUA"), &p).unwrap();
        assert_eq!((f.dll.file_name().unwrap().to_str().unwrap(), f.base.as_str()), ("crescue.dll", "crescue"));
        let p = Props { name: "Duel".into(), mission_type: "MultiLastOneStanding".into(), ..Default::default() };
        assert_eq!(new_files(Path::new("LUA"), &p).unwrap().dll, Path::new("LUA").join("Duel.dll"));
        assert!(new_files(Path::new("LUA"), &Props { name: "!!".into(), ..Default::default() }).is_none());
    }

    #[test]
    fn create_then_change_properties() {
        let dir = std::env::temp_dir().join(format!("op2luaeditor-props-{}", std::process::id()));
        let p = Props { name: "Iron Front".into(), map: "newworld.map".into(), players: 3, ..Default::default() };
        let files = new_files(&dir, &p).unwrap();
        create(&files, &p).unwrap();
        let d = MissionDll::open(&files.dll).unwrap();
        assert_eq!((d.level_desc.as_str(), d.map_name.as_str(), d.players), ("Iron Front", "newworld.map", 3));
        assert_eq!(d.mission_type, MissionType::Colony);
        let text = std::fs::read_to_string(&files.placement).unwrap();
        assert_eq!(Props::read(&text), p);
        assert_eq!(op2_formats::placement::parse_units(&text).units.len(), 7);
        assert_eq!(mission_dll(&files.placement).as_deref(), Some(files.dll.as_path()));

        let q = Props { name: "Steel Front".into(), map: "eden04.map".into(), mission_type: "Tutorial".into(), max_tech: 8, ..p.clone() };
        let summary = patch_mission_dll(&files.dll, &p, &q).unwrap();
        assert!(summary.contains("patched 3"), "{summary}");
        let d = MissionDll::open(&files.dll).unwrap();
        assert_eq!((d.level_desc.as_str(), d.map_name.as_str(), d.max_tech_level), ("Steel Front", "eden04.map", 8));
        assert_eq!(d.mission_type, MissionType::Tutorial);
        let rewritten = q.write_into(&text);
        assert_eq!(Props::read(&rewritten), q);
        assert_eq!(op2_formats::placement::parse_units(&rewritten).units.len(), 7);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn multiplayer_skeleton() {
        let p = Props { name: "Duel".into(), mission_type: "MultiLastOneStanding".into(), players: 4, ..Default::default() };
        let text = placement_skeleton("Duel", &p);
        assert_eq!(op2_formats::placement::parse_units(&text).units.len(), 4);
        assert!(!text.contains("players = {"));
        assert!(script_skeleton("Duel", &p).contains("mission.last_one_standing()"));
    }
}
