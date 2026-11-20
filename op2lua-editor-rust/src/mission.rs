//! An open mission: its `<Name>.placement.lua` (the layout the editor edits) and `<Name>.lua` (the logic, not edited
//! here), what the placement holds, and saving it back.

use std::path::{Path, PathBuf};

use op2_formats::placement::{self, MapObject, Region, UnitsTable};

/// The open mission. `units` and `regions` are what the editor changes; `loaded_text` is the file as last read
/// or written, what Save rewrites and what the unsaved-edits check compares with.
pub struct Mission {
    pub placement_path: PathBuf,
    /// `<Name>.lua` beside the placement, when there is one.
    pub script_path: Option<PathBuf>,
    /// The placement's `name = "..."`, else the file's base name.
    pub name: String,
    /// The placement's `map = "..."`.
    pub map_name: String,
    /// The placement file's text as last read or written.
    pub loaded_text: String,
    /// The units table (with each entry's own text), the regions, and the beacons, walls and markers (read only).
    pub units: UnitsTable,
    pub regions: Vec<Region>,
    pub objects: Vec<MapObject>,
}

/// The placement file of a mission picked by either of its files.
pub fn placement_for(picked: &Path) -> PathBuf {
    let s = picked.to_string_lossy();
    if s.to_ascii_lowercase().ends_with(".placement.lua") {
        return picked.to_path_buf();
    }
    let stem = s.strip_suffix(".lua").or_else(|| s.strip_suffix(".LUA")).unwrap_or(&s);
    PathBuf::from(format!("{stem}.placement.lua"))
}

/// The mission's base name: `Name` of `Name.placement.lua`.
pub fn base_name(placement: &Path) -> String {
    let file = placement.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
    let n = file.len().saturating_sub(".placement.lua".len());
    if file.to_ascii_lowercase().ends_with(".placement.lua") { file[..n].to_string() } else { file }
}

impl Mission {
    /// Opens a mission from either of its files.
    pub fn open(picked: &Path) -> Result<Self, String> {
        let placement_path = placement_for(picked);
        if !placement_path.is_file() {
            return Err(format!(
                "There is no placement file {}.\nAn OP2Lua mission needs <Name>.placement.lua beside <Name>.lua.",
                placement_path.display()
            ));
        }
        let text = std::fs::read_to_string(&placement_path)
            .map_err(|e| format!("Can't read {}: {e}", placement_path.display()))?;
        let map_name = placement::scalar_string(&text, "map")
            .map(|m| m.trim().to_string())
            .filter(|m| !m.is_empty())
            .ok_or_else(|| format!("{} names no map (map = \"eden01.map\").", placement_path.display()))?;
        let base = base_name(&placement_path);
        let script = placement_path.with_file_name(format!("{base}.lua"));
        let mut mission = Self {
            script_path: script.is_file().then_some(script),
            name: placement::scalar_string(&text, "name").filter(|n| !n.is_empty()).unwrap_or(base),
            map_name,
            placement_path,
            loaded_text: String::new(),
            units: UnitsTable::default(),
            regions: Vec::new(),
            objects: Vec::new(),
        };
        mission.load_text(text);
        Ok(mission)
    }

    /// A mission read from placement text, its file at `path` (for tests).
    #[cfg(test)]
    pub fn from_text(path: &Path, text: &str) -> Self {
        let mut m = Self {
            placement_path: path.to_path_buf(),
            script_path: None,
            name: "Test".into(),
            map_name: "test.map".into(),
            loaded_text: String::new(),
            units: UnitsTable::default(),
            regions: Vec::new(),
            objects: Vec::new(),
        };
        m.load_text(text.to_string());
        m
    }

    /// Reads the units, regions and map objects from the placement's text.
    pub fn load_text(&mut self, text: String) {
        self.units = placement::parse_units(&text);
        self.regions = placement::parse_regions(&text);
        self.objects = placement::parse_map_objects(&text);
        self.loaded_text = text;
    }

    /// The map to open: a file of that name beside the mission, else the name for the game's files to find.
    pub fn map_to_open(&self) -> String {
        let local = self.placement_path.with_file_name(&self.map_name);
        if local.is_file() { local.to_string_lossy().into_owned() } else { self.map_name.clone() }
    }

    /// The placement text with the editor's units and regions written into `text`.
    fn render(&self, text: &str) -> String {
        let nl = placement::new_line_of(text);
        let mut out = placement::replace_table(text, "regions", &placement::emit_regions(&self.regions, nl));
        if self.units.found || !self.units.units.is_empty() {
            out = placement::replace_table(&out, "units", &placement::emit_units(&self.units));
            out = placement::fix_beacon_after(&out, &self.units);
        }
        out
    }

    /// Whether the units or regions differ from the placement file's (as last read or written): the units as the
    /// text Save would write, the regions by name and corners (Save rewrites their table's layout).
    pub fn has_unsaved_edits(&self) -> bool {
        let text = &self.loaded_text;
        if self.units.found || !self.units.units.is_empty() {
            let units = placement::replace_table(text, "units", &placement::emit_units(&self.units));
            if placement::fix_beacon_after(&units, &self.units) != *text {
                return true;
            }
        }
        placement::parse_regions(text) != self.regions
    }

    /// The placement file's text on disk when it differs from what the editor last read or wrote.
    pub fn changed_on_disk(&self) -> Option<String> {
        let disk = std::fs::read_to_string(&self.placement_path).ok()?;
        (disk != self.loaded_text).then_some(disk)
    }

    /// Writes the units and regions into the placement file (the previous file kept as `<file>.bak`), then reads
    /// the file back so the next save starts from what is in it. Returns a summary.
    pub fn save(&mut self) -> Result<String, String> {
        let path = self.placement_path.clone();
        let current = std::fs::read_to_string(&path).unwrap_or_else(|_| self.loaded_text.clone());
        let updated = self.render(&current);
        let backup = PathBuf::from(format!("{}.bak", path.display()));
        if path.is_file() {
            std::fs::copy(&path, &backup).map_err(|e| format!("Can't back up to {}: {e}", backup.display()))?;
        }
        std::fs::write(&path, &updated).map_err(|e| format!("Can't write {}: {e}", path.display()))?;
        let (regions, units) = (self.regions.len(), self.units.units.len());
        self.load_text(updated);
        Ok(format!(
            "Saved {regions} region(s) and {units} unit(s) to {} (backup {}).",
            path.file_name().unwrap_or_default().to_string_lossy(),
            backup.file_name().unwrap_or_default().to_string_lossy()
        ))
    }

    /// Replaces the regions with another placement file's. Returns how many it had.
    pub fn import_regions(&mut self, other: &Path) -> Result<usize, String> {
        let text = std::fs::read_to_string(other).map_err(|e| format!("Can't read {}: {e}", other.display()))?;
        self.regions = placement::parse_regions(&text);
        Ok(self.regions.len())
    }

    /// Writes the regions or the units into another placement file (its table replaced, the rest kept; the previous
    /// file kept as `.bak`).
    pub fn write_table_into(&self, other: &Path, units: bool) -> Result<String, String> {
        let text = std::fs::read_to_string(other).map_err(|e| format!("Can't read {}: {e}", other.display()))?;
        let updated = if units {
            placement::replace_table(&text, "units", &placement::emit_units(&self.units))
        } else {
            placement::replace_table(&text, "regions", &placement::emit_regions(&self.regions, placement::new_line_of(&text)))
        };
        let bak = PathBuf::from(format!("{}.bak", other.display()));
        std::fs::copy(other, &bak).map_err(|e| format!("Can't back up {}: {e}", other.display()))?;
        std::fs::write(other, updated).map_err(|e| format!("Can't write {}: {e}", other.display()))?;
        let (n, what) = if units { (self.units.units.len(), "unit(s)") } else { (self.regions.len(), "region(s)") };
        Ok(format!(
            "Wrote {n} {what} into {} (backup {}).",
            other.file_name().unwrap_or_default().to_string_lossy(),
            bak.file_name().unwrap_or_default().to_string_lossy()
        ))
    }

    /// Units, beacons, walls, markers and regions on status-bar tile (x, y), one line per kind.
    pub fn tile_contents(&self, x: i32, y: i32) -> Vec<String> {
        let mut lines = Vec::new();
        let units: Vec<String> = self
            .units
            .units
            .iter()
            .filter(|u| u.has_at)
            .filter(|u| {
                let (x0, y0, x1, y1) = crate::units::footprint_tiles(&u.unit_type, u.x, u.y);
                (x0..=x1).contains(&x) && (y0..=y1).contains(&y)
            })
            .map(|u| {
                let name = u.name.as_ref().map(|n| format!(" \"{n}\"")).unwrap_or_default();
                format!("{} p{}{name}", u.unit_type, u.player)
            })
            .collect();
        if !units.is_empty() {
            lines.push(format!("Unit: {}", units.join(", ")));
        }
        let objects: Vec<String> = self
            .objects
            .iter()
            .filter(|o| o.x == x && o.y == y)
            .map(|o| {
                let what = match o.kind {
                    placement::ObjectKind::Beacon if o.obj_type == "MiningBeacon" => format!("{} beacon", o.ore),
                    placement::ObjectKind::Marker => format!("{} marker", o.obj_type),
                    _ => o.obj_type.clone(),
                };
                let name = o.name.as_ref().map(|n| format!(" \"{n}\"")).unwrap_or_default();
                format!("{what}{name}")
            })
            .collect();
        if !objects.is_empty() {
            lines.push(objects.join(", "));
        }
        let regions: Vec<&str> =
            self.regions.iter().filter(|r| r.contains(x, y)).map(|r| r.name.as_str()).collect();
        if !regions.is_empty() {
            lines.push(format!("In region: {}", regions.join(", ")));
        }
        lines
    }
}
