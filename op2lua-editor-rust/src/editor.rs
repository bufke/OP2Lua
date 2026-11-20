//! The mission layer over the map: drawing what the placement holds, and the unit and region tools.

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, PointerButton, Rect, Shape, Stroke, StrokeKind, Vec2, pos2,
};

use crate::mission::Mission;
use op2_formats::placement::{self, ObjectKind, Region, Unit};
use crate::units;
use crate::view::View;

/// What a left click or drag on the map does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Tool {
    /// Clicks select units, regions and tiles.
    Select,
    /// Click empty ground to place a unit; drag a unit to move it.
    Units,
    /// Drag on empty ground to draw a rectangle (a click makes a point); drag a region to move it, its handles to
    /// resize it.
    Regions,
}

/// A resize handle of a rectangle region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Handle {
    NW,
    N,
    NE,
    E,
    SE,
    S,
    SW,
    W,
}

enum Drag {
    None,
    Unit(usize),
    Draw { start: (i32, i32), current: (i32, i32) },
    MoveRegion { index: usize, start: (i32, i32), orig: Region },
    Resize { index: usize, handle: Handle },
}

const HANDLE_PX: f32 = 8.0;

/// A tube tile on the map, the walls table's and the ones structures lay alike.
const TUBE_COLOR: Color32 = Color32::from_rgba_unmultiplied_const(205, 170, 110, 170);

/// The tools' state: the tool, what is selected, what the unit tool places, the drag under way and the windows
/// the tools are waiting on. Selections are indexes into the mission's units and regions.
pub struct Editor {
    pub tool: Tool,
    pub selected_unit: Option<usize>,
    pub selected_region: Option<usize>,
    /// What the unit tool places.
    pub unit_type: String,
    /// The player (1-6) and weapon ("" for none) the unit tool places with.
    pub unit_player: i32,
    pub unit_weapon: String,
    /// Whether units (with their dozed borders), and beacons, walls and markers, are drawn.
    pub show_units: bool,
    pub show_objects: bool,
    drag: Drag,
    /// A region drawn and waiting for its name.
    pub naming: Option<(Region, String)>,
    /// Clear All asked for, waiting for the user's yes.
    pub confirm_clear: Option<Clear>,
}

/// What Clear All removes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Clear {
    Units,
    Regions,
}

impl Default for Editor {
    fn default() -> Self {
        Self {
            tool: Tool::Select,
            selected_unit: None,
            selected_region: None,
            unit_type: "Lynx".into(),
            unit_player: 1,
            unit_weapon: String::new(),
            show_units: true,
            show_objects: true,
            drag: Drag::None,
            naming: None,
            confirm_clear: None,
        }
    }
}

/// A unit's line in the Units list: type, weapon or cargo, player, tile and name.
fn describe_unit(u: &Unit) -> String {
    let w = if u.weapon.is_empty() { String::new() } else { format!(" {}", u.weapon) };
    let n = u.name.as_ref().map(|n| format!("  \"{n}\"")).unwrap_or_default();
    if !u.has_at {
        return format!("{}{w}  p{} (not on the map){n}", u.unit_type, u.player);
    }
    format!("{}{w}  p{} ({},{}){n}", u.unit_type, u.player, u.x, u.y)
}

/// A region's line in the Regions list: its name and its tile, or its two corners.
fn describe_region(r: &Region) -> String {
    if r.point {
        format!("{}  ({},{})", r.name, r.x1, r.y1)
    } else {
        format!("{}  ({},{})-({},{})", r.name, r.x1, r.y1, r.x2, r.y2)
    }
}

/// The topmost unit whose footprint covers status-bar tile (x, y).
fn unit_at(m: &Mission, x: i32, y: i32) -> Option<usize> {
    m.units.units.iter().enumerate().rev().find_map(|(i, u)| {
        if !u.has_at {
            return None;
        }
        let (x0, y0, x1, y1) = units::footprint_tiles(&u.unit_type, u.x, u.y);
        ((x0..=x1).contains(&x) && (y0..=y1).contains(&y)).then_some(i)
    })
}

fn region_at(m: &Mission, x: i32, y: i32) -> Option<usize> {
    m.regions.iter().enumerate().rev().find_map(|(i, r)| r.contains(x, y).then_some(i))
}

fn handle_rects(rect: Rect) -> [(Handle, Rect); 8] {
    let c = rect.center();
    let at = |x: f32, y: f32| Rect::from_center_size(pos2(x, y), Vec2::splat(HANDLE_PX));
    [
        (Handle::NW, at(rect.left(), rect.top())),
        (Handle::N, at(c.x, rect.top())),
        (Handle::NE, at(rect.right(), rect.top())),
        (Handle::E, at(rect.right(), c.y)),
        (Handle::SE, at(rect.right(), rect.bottom())),
        (Handle::S, at(c.x, rect.bottom())),
        (Handle::SW, at(rect.left(), rect.bottom())),
        (Handle::W, at(rect.left(), c.y)),
    ]
}

fn region_rect(view: &View, r: &Region) -> Rect {
    view.status_rect(r.x1, r.y1, r.x2, r.y2)
}

impl Editor {
    /// Forgets selections and drags (a mission was opened or closed).
    pub fn reset(&mut self) {
        self.selected_unit = None;
        self.selected_region = None;
        self.drag = Drag::None;
        self.naming = None;
    }

    /// Removes the selected unit (keeping the comments around it as `placement::delete_unit` does), else the
    /// selected region.
    pub fn delete_selected(&mut self, m: &mut Mission) {
        if let Some(i) = self.selected_unit.take().filter(|&i| i < m.units.units.len()) {
            placement::delete_unit(&mut m.units, i);
        } else if let Some(i) = self.selected_region.take().filter(|&i| i < m.regions.len()) {
            m.regions.remove(i);
        }
    }

    /// Removes every unit, last first, so the comments between them go as they do for single deletes.
    pub fn clear_units(&mut self, m: &mut Mission) {
        while !m.units.units.is_empty() {
            let last = m.units.units.len() - 1;
            placement::delete_unit(&mut m.units, last);
        }
        self.selected_unit = None;
    }

    /// Removes every region.
    pub fn clear_regions(&mut self, m: &mut Mission) {
        m.regions.clear();
        self.selected_region = None;
    }

    /// Handles the mouse on the canvas for the current tool. `w`, `h`: the map's size in tiles.
    pub fn handle_input(&mut self, ui: &egui::Ui, response: &egui::Response, view: &View, m: &mut Mission, w: i32, h: i32) {
        let clamp = |(x, y): (i32, i32)| (x.clamp(1, w), y.clamp(1, h));
        let on_map = |(x, y): (i32, i32)| (1..=w).contains(&x) && (1..=h).contains(&y);
        let hover = response.hover_pos().map(|p| view.status_tile_at(p));

        if response.drag_started_by(PointerButton::Primary) {
            let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or_default();
            let t = view.status_tile_at(origin);
            match self.tool {
                Tool::Units => {
                    if let Some(i) = unit_at(m, t.0, t.1) {
                        self.selected_unit = Some(i);
                        self.drag = Drag::Unit(i);
                    }
                }
                Tool::Regions => {
                    let handle = self.selected_region.filter(|&i| i < m.regions.len() && !m.regions[i].point).and_then(|i| {
                        let rect = region_rect(view, &m.regions[i]);
                        handle_rects(rect).into_iter().find(|(_, r)| r.contains(origin)).map(|(h, _)| (i, h))
                    });
                    if let Some((index, handle)) = handle {
                        self.drag = Drag::Resize { index, handle };
                    } else if let Some(index) = region_at(m, t.0, t.1) {
                        self.selected_region = Some(index);
                        self.drag = Drag::MoveRegion { index, start: t, orig: m.regions[index].clone() };
                    } else if on_map(t) {
                        self.drag = Drag::Draw { start: t, current: t };
                    }
                }
                Tool::Select => {}
            }
        }

        if response.dragged_by(PointerButton::Primary)
            && let Some(t) = hover.map(clamp)
        {
            match &mut self.drag {
                Drag::Unit(i) => {
                    if let Some(u) = m.units.units.get_mut(*i) {
                        (u.x, u.y) = t;
                    }
                }
                Drag::Draw { current, .. } => *current = t,
                Drag::MoveRegion { index, start, orig } => {
                    let (dw, dh) = (orig.x2 - orig.x1, orig.y2 - orig.y1);
                    let nx = (orig.x1 + t.0 - start.0).clamp(1, (w - dw).max(1));
                    let ny = (orig.y1 + t.1 - start.1).clamp(1, (h - dh).max(1));
                    if let Some(r) = m.regions.get_mut(*index) {
                        (r.x1, r.y1, r.x2, r.y2) = (nx, ny, nx + dw, ny + dh);
                    }
                }
                Drag::Resize { index, handle } => {
                    if let Some(r) = m.regions.get_mut(*index) {
                        let (x, y) = t;
                        match handle {
                            Handle::NW => (r.x1, r.y1) = (x, y),
                            Handle::N => r.y1 = y,
                            Handle::NE => (r.x2, r.y1) = (x, y),
                            Handle::E => r.x2 = x,
                            Handle::SE => (r.x2, r.y2) = (x, y),
                            Handle::S => r.y2 = y,
                            Handle::SW => (r.x1, r.y2) = (x, y),
                            Handle::W => r.x1 = x,
                        }
                    }
                }
                Drag::None => {}
            }
        }

        if response.drag_stopped_by(PointerButton::Primary) {
            match std::mem::replace(&mut self.drag, Drag::None) {
                Drag::Draw { start, current } => {
                    let r = Region {
                        name: String::new(),
                        point: start == current,
                        x1: start.0.min(current.0),
                        y1: start.1.min(current.1),
                        x2: start.0.max(current.0),
                        y2: start.1.max(current.1),
                    };
                    self.start_naming(m, r);
                }
                Drag::MoveRegion { index, .. } | Drag::Resize { index, .. } => {
                    if let Some(r) = m.regions.get_mut(index) {
                        r.normalize();
                    }
                }
                _ => {}
            }
        }

        if response.clicked_by(PointerButton::Primary)
            && let Some(t) = response.interact_pointer_pos().map(|p| view.status_tile_at(p))
        {
            match self.tool {
                Tool::Units if on_map(t) => match unit_at(m, t.0, t.1) {
                    Some(i) => self.selected_unit = Some(i),
                    None => {
                        let mut weapon = self.unit_weapon.clone();
                        units::ensure_weapon(&self.unit_type, &mut weapon);
                        m.units.units.push(Unit::new(&self.unit_type, self.unit_player, t.0, t.1, &weapon));
                        self.selected_unit = Some(m.units.units.len() - 1);
                    }
                },
                Tool::Regions if on_map(t) => match region_at(m, t.0, t.1) {
                    Some(i) => self.selected_region = Some(i),
                    None => self.start_naming(m, Region { name: String::new(), point: true, x1: t.0, y1: t.1, x2: t.0, y2: t.1 }),
                },
                Tool::Select => {
                    self.selected_unit = unit_at(m, t.0, t.1);
                    self.selected_region = if self.selected_unit.is_none() { region_at(m, t.0, t.1) } else { None };
                }
                _ => {}
            }
        }
    }

    fn start_naming(&mut self, m: &Mission, r: Region) {
        let mut n = m.regions.len() + 1;
        while m.regions.iter().any(|r| r.name == format!("region{n}")) {
            n += 1;
        }
        self.naming = Some((r, format!("region{n}")));
    }

    /// The "clear all units / regions?" window.
    pub fn clear_window(&mut self, ctx: &egui::Context, m: &mut Mission) {
        let Some(what) = self.confirm_clear else { return };
        let (title, text) = match what {
            Clear::Units => ("Clear all units", format!("Remove all {} units?", m.units.units.len())),
            Clear::Regions => ("Clear all regions", format!("Remove all {} regions?", m.regions.len())),
        };
        let mut answer = None;
        egui::Window::new(title).collapsible(false).resizable(false).show(ctx, |ui| {
            ui.label(text);
            ui.small("Nothing is written until you save.");
            ui.horizontal(|ui| {
                if ui.button("Remove them").clicked() {
                    answer = Some(true);
                }
                if ui.button("Cancel").clicked() {
                    answer = Some(false);
                }
            });
        });
        match answer {
            Some(true) => {
                match what {
                    Clear::Units => self.clear_units(m),
                    Clear::Regions => self.clear_regions(m),
                }
                self.confirm_clear = None;
            }
            Some(false) => self.confirm_clear = None,
            None => {}
        }
    }

    /// The "name the new region" window. Adds the region on OK.
    pub fn naming_window(&mut self, ctx: &egui::Context, m: &mut Mission) {
        let Some((region, name)) = &mut self.naming else { return };
        let mut done = None;
        egui::Window::new("New region").collapsible(false).resizable(false).show(ctx, |ui| {
            ui.label(describe_region(region).trim_start().to_string());
            ui.horizontal(|ui| {
                ui.label("Name:");
                let edit = ui.text_edit_singleline(name);
                edit.request_focus();
                if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    done = Some(true);
                }
            });
            ui.small("A Lua name: letters, digits and _, not starting with a digit.");
            ui.horizontal(|ui| {
                if ui.button("OK").clicked() {
                    done = Some(true);
                }
                if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    done = Some(false);
                }
            });
        });
        match done {
            Some(true) if valid_lua_name(name.trim()) => {
                let (mut r, name) = self.naming.take().unwrap();
                r.name = name.trim().to_string();
                m.regions.push(r);
                self.selected_region = Some(m.regions.len() - 1);
            }
            Some(false) => self.naming = None,
            _ => {}
        }
    }

    /// Draws regions, dozed borders, walls, beacons, markers and units over the map.
    pub fn draw(&self, painter: &egui::Painter, view: &View, m: &Mission) {
        let t = view.tile_size();
        let font = FontId::proportional(12.0);

        for (i, r) in m.regions.iter().enumerate() {
            let col = units::REGION_COLORS[i % units::REGION_COLORS.len()];
            let rect = region_rect(view, r);
            let selected = self.selected_region == Some(i);
            painter.rect_filled(rect, CornerRadius::ZERO, col.gamma_multiply(0.24));
            painter.rect_stroke(rect, CornerRadius::ZERO, Stroke::new(if selected { 3.0 } else { 2.0 }, col), StrokeKind::Inside);
            if r.point {
                painter.line_segment([rect.left_top(), rect.right_bottom()], Stroke::new(1.0, col));
                painter.line_segment([rect.right_top(), rect.left_bottom()], Stroke::new(1.0, col));
            }
            let galley = painter.layout_no_wrap(r.name.clone(), font.clone(), col);
            let label = Rect::from_min_size(rect.left_top() - egui::vec2(0.0, galley.size().y), galley.size() + egui::vec2(4.0, 0.0));
            painter.rect_filled(label, CornerRadius::ZERO, Color32::from_black_alpha(180));
            painter.galley(label.min + egui::vec2(2.0, 0.0), galley, col);
            if selected && !r.point && self.tool == Tool::Regions {
                for (_, h) in handle_rects(rect) {
                    painter.rect_filled(h, CornerRadius::ZERO, Color32::WHITE);
                    painter.rect_stroke(h, CornerRadius::ZERO, Stroke::new(1.0, Color32::BLACK), StrokeKind::Inside);
                }
            }
        }

        if self.show_units {
            // The dozed ring around every structure, each tile filled once.
            let mut dozed = std::collections::BTreeSet::new();
            for u in m.units.units.iter().filter(|u| u.has_at && units::has_dozed_border(&u.unit_type)) {
                let (x0, y0, x1, y1) = units::footprint_tiles(&u.unit_type, u.x, u.y);
                for x in x0 - 1..=x1 + 1 {
                    for y in y0 - 1..=y1 + 1 {
                        dozed.insert((x, y));
                    }
                }
            }
            for (x, y) in dozed {
                painter.rect_filled(view.status_rect(x, y, x, y), CornerRadius::ZERO, Color32::from_rgba_unmultiplied(72, 66, 60, 150));
            }
        }

        if self.show_units {
            // The two tubes the game lays beside each structure with tube connections when it is placed.
            for u in m.units.units.iter().filter(|u| u.has_at) {
                for (x, y) in units::dropped_tubes(&u.unit_type, u.x, u.y).into_iter().flatten() {
                    painter.rect_filled(view.status_rect(x, y, x, y), CornerRadius::ZERO, TUBE_COLOR);
                }
            }
        }

        if self.show_objects {
            for o in m.objects.iter().filter(|o| o.kind == ObjectKind::Wall) {
                let col = match o.obj_type.as_str() {
                    "Tube" => TUBE_COLOR,
                    "LavaWall" => Color32::from_rgba_unmultiplied(230, 90, 30, 190),
                    "MicrobeWall" => Color32::from_rgba_unmultiplied(110, 160, 50, 190),
                    _ => Color32::from_rgba_unmultiplied(120, 120, 120, 190),
                };
                painter.rect_filled(view.status_rect(o.x, o.y, o.x, o.y), CornerRadius::ZERO, col);
            }
            for o in m.objects.iter().filter(|o| o.kind != ObjectKind::Wall) {
                let r = view.status_rect(o.x, o.y, o.x, o.y);
                let c = r.center();
                let h = (t * 0.45).max(3.0);
                if o.kind == ObjectKind::Beacon {
                    let (label, col) = match (o.obj_type.as_str(), o.ore.as_str()) {
                        ("MagmaVent", _) => ("V", Color32::from_rgb(255, 69, 0)),
                        ("Fumarole", _) => ("F", Color32::from_rgb(147, 112, 219)),
                        (_, "Common") => ("C", Color32::from_rgb(255, 215, 0)),
                        (_, "Rare") => ("R", Color32::from_rgb(0, 191, 255)),
                        _ => ("?", Color32::from_rgb(245, 245, 245)),
                    };
                    let pts = vec![pos2(c.x, c.y - h), pos2(c.x + h, c.y), pos2(c.x, c.y + h), pos2(c.x - h, c.y)];
                    painter.add(Shape::convex_polygon(pts, col.gamma_multiply(0.8), Stroke::new(1.0, Color32::BLACK)));
                    if t >= 12.0 {
                        painter.text(c, Align2::CENTER_CENTER, label, FontId::proportional((t * 0.35).max(8.0)), Color32::BLACK);
                    }
                } else {
                    painter.circle_stroke(c, h, Stroke::new((t / 12.0).max(1.0), Color32::from_rgb(255, 0, 255)));
                    if let Some(name) = &o.name {
                        painter.text(r.right_top(), Align2::LEFT_TOP, name, font.clone(), Color32::from_rgb(255, 0, 255));
                    }
                }
            }
        }

        if self.show_units {
            let label_font = FontId::proportional((t * 0.3).clamp(7.0, 12.0));
            for (i, u) in m.units.units.iter().enumerate().filter(|(_, u)| u.has_at) {
                let (x0, y0, x1, y1) = units::footprint_tiles(&u.unit_type, u.x, u.y);
                let rect = view.status_rect(x0, y0, x1, y1);
                painter.rect_filled(rect, CornerRadius::ZERO, units::player_color(u.player).gamma_multiply(0.67));
                let stroke = if self.selected_unit == Some(i) { Stroke::new(3.0, Color32::WHITE) } else { Stroke::new(1.0, Color32::BLACK) };
                painter.rect_stroke(rect, CornerRadius::ZERO, stroke, StrokeKind::Inside);
                if t >= 10.0 {
                    let label = units::abbreviation(&u.unit_type);
                    painter.text(rect.center() + egui::vec2(1.0, 1.0), Align2::CENTER_CENTER, &label, label_font.clone(), Color32::BLACK);
                    painter.text(rect.center(), Align2::CENTER_CENTER, &label, label_font.clone(), Color32::WHITE);
                }
            }
        }

        if let Drag::Draw { start, current } = self.drag {
            let rect = view.status_rect(start.0, start.1, current.0, current.1);
            for side in [
                [rect.left_top(), rect.right_top()],
                [rect.right_top(), rect.right_bottom()],
                [rect.right_bottom(), rect.left_bottom()],
                [rect.left_bottom(), rect.left_top()],
            ] {
                painter.extend(Shape::dashed_line(&side, Stroke::new(2.0, Color32::WHITE), 6.0, 4.0));
            }
        }
    }

    /// The tool bar: the tool, and what the unit tool places.
    pub fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Tool:");
            ui.selectable_value(&mut self.tool, Tool::Select, "Select");
            ui.selectable_value(&mut self.tool, Tool::Units, "Units");
            ui.selectable_value(&mut self.tool, Tool::Regions, "Regions");
            ui.separator();
            match self.tool {
                Tool::Units => {
                    unit_type_combo(ui, "place-type", &mut self.unit_type);
                    units::ensure_weapon(&self.unit_type, &mut self.unit_weapon);
                    player_combo(ui, "place-player", &mut self.unit_player);
                    weapon_combo(ui, "place-weapon", &self.unit_type, &mut self.unit_weapon);
                    ui.weak("click: place, drag: move");
                }
                Tool::Regions => {
                    ui.weak("drag: rectangle, click: point; drag a region to move it, its handles to resize");
                }
                Tool::Select => {
                    ui.weak("click a unit or region to select it; Delete removes it");
                }
            }
            ui.separator();
            ui.checkbox(&mut self.show_units, "Units");
            ui.checkbox(&mut self.show_objects, "Beacons, walls, markers");
        });
    }

    /// The side panel's mission part: the selected unit or region, and the lists.
    pub fn panel(&mut self, ui: &mut egui::Ui, m: &mut Mission) {
        ui.label(egui::RichText::new(&m.name).heading());
        ui.small(m.placement_path.display().to_string());
        if let Some(s) = &m.script_path {
            ui.small(format!("Script: {}", s.file_name().unwrap_or_default().to_string_lossy()));
        }
        let (b, w, mk) = m.objects.iter().fold((0, 0, 0), |(b, w, mk), o| match o.kind {
            ObjectKind::Beacon => (b + 1, w, mk),
            ObjectKind::Wall => (b, w + 1, mk),
            ObjectKind::Marker => (b, w, mk + 1),
        });
        ui.small(format!("{b} beacon(s), {w} wall(s)/tube(s), {mk} marker(s)"));

        if let Some(i) = self.selected_unit.filter(|&i| i < m.units.units.len()) {
            ui.separator();
            ui.label(egui::RichText::new("Selected unit").strong());
            let u = &mut m.units.units[i];
            egui::Grid::new("unit-edit").num_columns(2).show(ui, |ui| {
                ui.label("Type");
                if unit_type_combo(ui, "edit-type", &mut u.unit_type) {
                    units::ensure_weapon(&u.unit_type, &mut u.weapon);
                }
                ui.end_row();
                ui.label("Player");
                player_combo(ui, "edit-player", &mut u.player);
                ui.end_row();
                ui.label("Weapon");
                weapon_combo(ui, "edit-weapon", &u.unit_type, &mut u.weapon);
                ui.end_row();
                if u.has_at {
                    ui.label("At");
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut u.x).range(1..=1024));
                        ui.add(egui::DragValue::new(&mut u.y).range(1..=1024));
                    });
                    ui.end_row();
                }
                if let Some(n) = &u.name {
                    ui.label("Name");
                    ui.label(n);
                    ui.end_row();
                }
            });
            if ui.button("Delete unit").clicked() {
                self.delete_selected(m);
            }
        } else if let Some(i) = self.selected_region.filter(|&i| i < m.regions.len()) {
            ui.separator();
            ui.label(egui::RichText::new("Selected region").strong());
            let r = &mut m.regions[i];
            egui::Grid::new("region-edit").num_columns(2).show(ui, |ui| {
                ui.label("Name");
                let mut name = r.name.clone();
                if ui.text_edit_singleline(&mut name).changed() && valid_lua_name(&name) {
                    r.name = name;
                }
                ui.end_row();
                ui.label(if r.point { "At" } else { "From" });
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut r.x1).range(1..=1024));
                    ui.add(egui::DragValue::new(&mut r.y1).range(1..=1024));
                });
                ui.end_row();
                if !r.point {
                    ui.label("To");
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut r.x2).range(1..=1024));
                        ui.add(egui::DragValue::new(&mut r.y2).range(1..=1024));
                    });
                    ui.end_row();
                }
            });
            if r.point {
                (r.x2, r.y2) = (r.x1, r.y1);
            }
            if ui.button("Delete region").clicked() {
                self.delete_selected(m);
            }
        }

        ui.separator();
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(format!("Units ({})", m.units.units.len())).strong());
            if !m.units.units.is_empty() && ui.small_button("Clear all").clicked() {
                self.confirm_clear = Some(Clear::Units);
            }
        });
        egui::ScrollArea::vertical().id_salt("units-list").max_height(220.0).show(ui, |ui| {
            for (i, u) in m.units.units.iter().enumerate() {
                if ui.selectable_label(self.selected_unit == Some(i), describe_unit(u)).clicked() {
                    self.selected_unit = Some(i);
                    self.selected_region = None;
                }
            }
        });
        ui.separator();
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(format!("Regions ({})", m.regions.len())).strong());
            if !m.regions.is_empty() && ui.small_button("Clear all").clicked() {
                self.confirm_clear = Some(Clear::Regions);
            }
        });
        egui::ScrollArea::vertical().id_salt("regions-list").max_height(160.0).show(ui, |ui| {
            for (i, r) in m.regions.iter().enumerate() {
                if ui.selectable_label(self.selected_region == Some(i), describe_region(r)).clicked() {
                    self.selected_region = Some(i);
                    self.selected_unit = None;
                }
            }
        });
    }
}

/// Whether `s` can be a key in the regions table as written (`name = { ... }`): a Lua name, not a keyword.
pub fn valid_lua_name(s: &str) -> bool {
    const KEYWORDS: &[&str] = &[
        "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "goto", "if", "in", "local", "nil",
        "not", "or", "repeat", "return", "then", "true", "until", "while",
    ];
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !KEYWORDS.contains(&s)
}

/// The unit type picker; true when the type changed.
fn unit_type_combo(ui: &mut egui::Ui, id: &str, value: &mut String) -> bool {
    let before = value.clone();
    egui::ComboBox::from_id_salt(id).selected_text(value.as_str()).height(400.0).show_ui(ui, |ui| {
        ui.label(egui::RichText::new("Vehicles").strong());
        for v in units::VEHICLES {
            ui.selectable_value(value, v.to_string(), *v);
        }
        ui.separator();
        ui.label(egui::RichText::new("Structures").strong());
        for s in units::STRUCTURES {
            ui.selectable_value(value, s.to_string(), *s);
        }
    });
    *value != before
}

fn player_combo(ui: &mut egui::Ui, id: &str, value: &mut i32) {
    egui::ComboBox::from_id_salt(id).selected_text(format!("Player {value}")).show_ui(ui, |ui| {
        for p in 1..=6 {
            ui.selectable_value(value, p, format!("Player {p}"));
        }
    });
}

/// The weapon picker. A unit that needs a weapon isn't offered "no weapon"; one loaded without it shows in red.
fn weapon_combo(ui: &mut egui::Ui, id: &str, unit_type: &str, value: &mut String) {
    let armed = units::needs_weapon(unit_type);
    let shown = match (value.is_empty(), armed) {
        (true, true) => egui::RichText::new("(needs a weapon)").color(egui::Color32::RED),
        (true, false) => egui::RichText::new("(no weapon)"),
        (false, _) => egui::RichText::new(value.as_str()),
    };
    egui::ComboBox::from_id_salt(id).selected_text(shown).show_ui(ui, |ui| {
        for w in units::WEAPONS.iter().filter(|w| !(armed && w.is_empty())) {
            ui.selectable_value(value, w.to_string(), if w.is_empty() { "(no weapon)" } else { w });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Event, Id, Modifiers, Pos2, RawInput, Sense, pos2, vec2};

    const TEXT: &str = "return {\n  map = \"test.map\",\n  units = {\n    { type = \"Lynx\", player = 1, at = { 10, 10 } },\n  },\n  regions = { base = { 20, 20, 25, 24 } },\n}\n";
    /// Screen pixels per tile in the tests (zoom 0.25).
    const T: f32 = 8.0;

    struct Harness {
        ctx: egui::Context,
        time: f64,
        editor: Editor,
        mission: Mission,
        view: View,
    }

    impl Harness {
        fn new(tool: Tool) -> Self {
            Self {
                ctx: egui::Context::default(),
                time: 0.0,
                editor: Editor { tool, ..Default::default() },
                mission: Mission::from_text(std::path::Path::new("test.placement.lua"), TEXT),
                view: View::for_test(Rect::from_min_size(Pos2::ZERO, vec2(64.0 * T, 64.0 * T)), 0.25),
            }
        }

        fn frame(&mut self, events: Vec<Event>) {
            self.time += 0.05;
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1024.0, 768.0))),
                time: Some(self.time),
                events,
                ..Default::default()
            };
            let (editor, mission, view) = (&mut self.editor, &mut self.mission, &self.view);
            let mut out = self.ctx.run_ui(input, |ui| {
                let response = ui.interact(view.canvas(), Id::unique("canvas"), Sense::click_and_drag());
                editor.handle_input(ui, &response, view, mission, 64, 64);
            });
            out.textures_delta.clear(); // no renderer to take the font texture
        }

        fn button(&mut self, p: Pos2, pressed: bool) {
            self.frame(vec![Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }]);
        }

        fn click(&mut self, tile: (i32, i32)) {
            let p = at(tile);
            self.frame(vec![Event::PointerMoved(p)]);
            self.button(p, true);
            self.button(p, false);
            self.frame(vec![]);
        }

        fn drag(&mut self, from: (i32, i32), to: (i32, i32)) {
            let (a, b) = (at(from), at(to));
            self.frame(vec![Event::PointerMoved(a)]);
            self.button(a, true);
            for k in 1..=4 {
                self.frame(vec![Event::PointerMoved(a + (b - a) * (k as f32 / 4.0))]);
            }
            self.button(b, false);
            self.frame(vec![]);
        }
    }

    /// The screen centre of status-bar tile (x, y).
    fn at((x, y): (i32, i32)) -> Pos2 {
        pos2((x - 1) as f32 * T + T / 2.0, (y - 1) as f32 * T + T / 2.0)
    }

    #[test]
    fn unit_tool_places_and_moves() {
        let mut h = Harness::new(Tool::Units);
        h.editor.unit_type = "Tiger".into();
        h.editor.unit_weapon = "RailGun".into();
        h.click((5, 6));
        assert_eq!(h.mission.units.units.len(), 2);
        let u = &h.mission.units.units[1];
        assert_eq!((u.unit_type.as_str(), u.x, u.y, u.weapon.as_str()), ("Tiger", 5, 6, "RailGun"));
        assert_eq!(h.editor.selected_unit, Some(1));
        h.drag((10, 10), (14, 12));
        let u = &h.mission.units.units[0];
        assert_eq!((u.x, u.y), (14, 12));
        assert!(h.mission.has_unsaved_edits());
    }

    #[test]
    fn clear_all_keeps_the_file_tidy() {
        let mut h = Harness::new(Tool::Select);
        h.editor.clear_units(&mut h.mission);
        h.editor.clear_regions(&mut h.mission);
        assert!(h.mission.units.units.is_empty() && h.mission.regions.is_empty());
        let text = placement::replace_table(TEXT, "units", &placement::emit_units(&h.mission.units));
        assert!(placement::parse_units(&text).units.is_empty());
        assert!(text.contains("units = {"), "{text}");
    }

    #[test]
    fn tanks_always_get_a_weapon() {
        let mut h = Harness::new(Tool::Units);
        h.editor.unit_type = "Panther".into();
        h.editor.unit_weapon = String::new();
        h.click((5, 6));
        assert_eq!(h.mission.units.units[1].weapon, units::DEFAULT_WEAPON);
        h.editor.unit_type = "ConVec".into();
        h.click((7, 6));
        assert_eq!(h.mission.units.units[2].weapon, "");
    }

    #[test]
    fn region_tool_draws_moves_and_resizes() {
        let mut h = Harness::new(Tool::Regions);
        h.drag((3, 4), (8, 9));
        let (r, name) = h.editor.naming.clone().expect("a drawn region waits for its name");
        assert_eq!((r.point, r.x1, r.y1, r.x2, r.y2, name.as_str()), (false, 3, 4, 8, 9, "region2"));
        h.editor.naming = None;

        h.click((40, 41));
        let (r, _) = h.editor.naming.take().unwrap();
        assert!(r.point && (r.x1, r.y1) == (40, 41));

        h.drag((21, 21), (31, 26));
        assert_eq!(h.mission.regions[0], Region { name: "base".into(), point: false, x1: 30, y1: 25, x2: 35, y2: 29 });
        assert_eq!(h.editor.selected_region, Some(0));

        // The south-east handle sits on the bottom-right corner of the selected region.
        let corner = h.view.status_rect(30, 25, 35, 29).right_bottom();
        let target = at((38, 33));
        h.frame(vec![Event::PointerMoved(corner)]);
        h.button(corner, true);
        for k in 1..=4 {
            h.frame(vec![Event::PointerMoved(corner + (target - corner) * (k as f32 / 4.0))]);
        }
        h.button(target, false);
        h.frame(vec![]);
        let r = &h.mission.regions[0];
        assert_eq!((r.x1, r.y1, r.x2, r.y2), (30, 25, 38, 33));
    }

    #[test]
    fn select_tool_selects_and_deletes() {
        let mut h = Harness::new(Tool::Select);
        h.click((10, 10));
        assert_eq!(h.editor.selected_unit, Some(0));
        h.editor.delete_selected(&mut h.mission);
        assert!(h.mission.units.units.is_empty());
        h.click((22, 22));
        assert_eq!(h.editor.selected_region, Some(0));
        h.editor.delete_selected(&mut h.mission);
        assert!(h.mission.regions.is_empty());
    }

    #[test]
    fn save_writes_and_backs_up() {
        let dir = std::env::temp_dir().join(format!("op2luaeditor-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Save.placement.lua");
        std::fs::write(&path, TEXT).unwrap();
        let mut m = Mission::from_text(&path, TEXT);
        m.units.units.push(Unit::new("Scout", 2, 3, 4, ""));
        m.regions[0].x2 = 26;
        m.save().unwrap();
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(saved.contains("type = \"Scout\", player = 2, at = { 3, 4 }"), "{saved}");
        assert!(saved.contains("base = { 20, 20, 26, 24 }"), "{saved}");
        assert_eq!(std::fs::read_to_string(dir.join("Save.placement.lua.bak")).unwrap(), TEXT);
        assert!(!m.has_unsaved_edits());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
