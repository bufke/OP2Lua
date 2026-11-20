//! Cell type names and colours for the map views.

use eframe::egui::Color32;
use op2_formats::CellType;

/// How the map is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum RenderMode {
    /// The tiles as the game draws them.
    GameMap,
    /// The tiles under a see-through colour for how fast units cross each one.
    Passability,
    /// The passability overlay with each tile's cell type named (when a tile is big enough to read).
    PassabilityNamed,
    /// Flat colours, one per kind of cell.
    CellTypes,
    /// Flat colours, one per cell type, with the type's name on each tile when it's big enough to read.
    CellTypesNamed,
}

impl RenderMode {
    /// Every view, in the View menu's order.
    pub const ALL: [RenderMode; 5] = [
        RenderMode::GameMap,
        RenderMode::Passability,
        RenderMode::PassabilityNamed,
        RenderMode::CellTypes,
        RenderMode::CellTypesNamed,
    ];

    /// Whether the view names each tile's cell type.
    pub fn named(self) -> bool {
        matches!(self, RenderMode::PassabilityNamed | RenderMode::CellTypesNamed)
    }

    /// The view's name in the View menu and the status bar.
    pub fn label(self) -> &'static str {
        match self {
            RenderMode::GameMap => "Game map",
            RenderMode::Passability => "Passability overlay",
            RenderMode::PassabilityNamed => "Passability overlay, named",
            RenderMode::CellTypes => "Cell types",
            RenderMode::CellTypesNamed => "Cell types, named",
        }
    }

    /// The colour a cell gets in this mode; None in the game map view.
    pub fn cell_color(self, ct: CellType) -> Option<Color32> {
        match self {
            RenderMode::GameMap => None,
            RenderMode::Passability | RenderMode::PassabilityNamed => Some(passability_color(ct).gamma_multiply(0.45)),
            RenderMode::CellTypes => Some(basic_color(ct)),
            RenderMode::CellTypesNamed => Some(detailed_color(ct)),
        }
    }
}

/// The cell type's full name, as the game's `CellType` enum names it.
pub fn name(ct: CellType) -> String {
    match ct {
        CellType::Padding(v) => format!("Padding{v}"),
        other => format!("{other:?}"),
    }
}

/// A short name that fits on a tile.
pub fn short_name(ct: CellType) -> &'static str {
    use CellType::*;
    match ct {
        FastPassible1 => "Fast1",
        FastPassible2 => "Fast2",
        MediumPassible1 => "Med1",
        MediumPassible2 => "Med2",
        SlowPassible1 => "Slow1",
        SlowPassible2 => "Slow2",
        Impassible1 => "Imp1",
        Impassible2 => "Imp2",
        NorthCliffs => "NCliff",
        CliffsHighSide => "CliffH",
        CliffsLowSide => "CliffL",
        VentsAndFumaroles => "Vent",
        DozedArea => "Dozed",
        Rubble => "Rubble",
        NormalWall => "Wall",
        MicrobeWall => "MWall",
        LavaWall => "LWall",
        Tube => "Tube",
        Padding(_) => "Pad",
    }
}

/// Passability: green fast, yellow medium, orange slow, red blocked, cyan tubes.
fn passability_color(ct: CellType) -> Color32 {
    use CellType::*;
    match ct {
        Impassible1 | Impassible2 | NorthCliffs | CliffsHighSide | CliffsLowSide | Rubble | NormalWall
        | MicrobeWall | LavaWall => Color32::from_rgb(192, 0, 0),
        SlowPassible1 | SlowPassible2 | VentsAndFumaroles => Color32::from_rgb(255, 128, 0),
        MediumPassible1 | MediumPassible2 => Color32::from_rgb(255, 255, 0),
        FastPassible1 | FastPassible2 | DozedArea => Color32::from_rgb(0, 255, 0),
        Tube => Color32::from_rgb(0, 255, 255),
        Padding(_) => Color32::WHITE,
    }
}

fn basic_color(ct: CellType) -> Color32 {
    use CellType::*;
    match ct {
        FastPassible1 | FastPassible2 => Color32::from_rgb(144, 238, 144),
        SlowPassible1 | SlowPassible2 => Color32::from_rgb(255, 255, 0),
        MediumPassible1 | MediumPassible2 => Color32::from_rgb(128, 128, 0),
        Impassible1 | Impassible2 => Color32::from_rgb(139, 0, 0),
        NorthCliffs | CliffsHighSide | CliffsLowSide => Color32::from_rgb(160, 82, 45),
        VentsAndFumaroles => Color32::from_rgb(255, 165, 0),
        DozedArea => Color32::from_rgb(211, 211, 211),
        Rubble => Color32::from_rgb(169, 169, 169),
        NormalWall => Color32::from_rgb(128, 128, 128),
        MicrobeWall => Color32::from_rgb(128, 0, 128),
        LavaWall => Color32::from_rgb(255, 69, 0),
        Tube => Color32::from_rgb(0, 255, 255),
        Padding(_) => Color32::WHITE,
    }
}

fn detailed_color(ct: CellType) -> Color32 {
    use CellType::*;
    match ct {
        FastPassible1 => Color32::from_rgb(0, 255, 0),
        FastPassible2 => Color32::from_rgb(0, 200, 0),
        MediumPassible1 => Color32::from_rgb(255, 255, 0),
        MediumPassible2 => Color32::from_rgb(210, 210, 0),
        SlowPassible1 => Color32::from_rgb(255, 128, 0),
        SlowPassible2 => Color32::from_rgb(210, 100, 0),
        Impassible1 => Color32::from_rgb(255, 0, 0),
        Impassible2 => Color32::from_rgb(200, 0, 0),
        NorthCliffs => Color32::from_rgb(139, 69, 19),
        CliffsHighSide => Color32::from_rgb(160, 90, 40),
        CliffsLowSide => Color32::from_rgb(115, 55, 15),
        VentsAndFumaroles => Color32::from_rgb(128, 0, 128),
        DozedArea => Color32::from_rgb(192, 192, 192),
        Rubble => Color32::from_rgb(64, 64, 64),
        NormalWall => Color32::from_rgb(128, 128, 128),
        MicrobeWall => Color32::from_rgb(128, 0, 255),
        LavaWall => Color32::from_rgb(255, 69, 0),
        Tube => Color32::from_rgb(0, 255, 255),
        Padding(_) => Color32::WHITE,
    }
}

/// Black or white, whichever reads better on `bg`.
pub fn text_color_on(bg: Color32) -> Color32 {
    let l = 0.299 * bg.r() as f32 + 0.587 * bg.g() as f32 + 0.114 * bg.b() as f32;
    if l > 140.0 { Color32::BLACK } else { Color32::WHITE }
}
