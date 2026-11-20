//! The map canvas: pan, zoom, and drawing the visible tiles straight from the tileset textures (no whole-map
//! bitmap: a 512 x 256 map would need 512 MB).

use eframe::egui::{
    self, Align2, Color32, FontId, Mesh, PointerButton, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2, pos2, vec2,
};
use op2_formats::TILE_SIZE;

use crate::celltype::{self, RenderMode};
use crate::mapdoc::MapDoc;

/// The zoom steps the mouse wheel moves through (1.0 = 32 screen pixels per tile).
const ZOOMS: [f32; 11] = [0.05, 0.1, 0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 2.0, 3.0, 4.0];

/// Below this many screen pixels per tile the overview texture is drawn instead of tiles.
const OVERVIEW_BELOW_PX: f32 = 8.0;
/// Grid lines and tile names need this many pixels per tile.
const GRID_FROM_PX: f32 = 12.0;
const NAMES_FROM_PX: f32 = 30.0;

/// Gray margin allowed around a map bigger than the canvas.
const PAN_MARGIN: f32 = 64.0;

/// How the map is shown: zoom, position, grid and view mode.
pub struct View {
    /// 1.0 = 32 screen pixels per tile.
    pub zoom: f32,
    /// Where the map's top-left corner sits, relative to the canvas's top-left.
    pub offset: Vec2,
    pub show_grid: bool,
    pub mode: RenderMode,
    /// Set when a map loads: the next frame centres it (the canvas size isn't known before).
    pub center_pending: bool,
    /// The last canvas, for centring from a menu.
    canvas: Rect,
}

/// What happened on the canvas this frame, in map tiles (0-based, file coordinates).
#[derive(Default)]
pub struct CanvasEvents {
    /// The tile under the pointer, and the tile clicked this frame.
    pub hovered: Option<(u32, u32)>,
    pub clicked: Option<(u32, u32)>,
}

impl Default for View {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            offset: Vec2::ZERO,
            show_grid: false,
            mode: RenderMode::GameMap,
            center_pending: true,
            canvas: Rect::NOTHING,
        }
    }
}

impl View {
    /// A view of a canvas at `rect`, zoomed to `zoom`, with the map's corner at the canvas's (for tests).
    #[cfg(test)]
    pub fn for_test(rect: Rect, zoom: f32) -> Self {
        Self { canvas: rect, zoom, center_pending: false, ..Default::default() }
    }

    fn tile_px(&self) -> f32 {
        TILE_SIZE as f32 * self.zoom
    }

    fn map_size(&self, doc: &MapDoc) -> Vec2 {
        vec2(doc.width() as f32, doc.height() as f32) * self.tile_px()
    }

    /// The map tile under a screen position, if it is on the map.
    fn tile_at(&self, doc: &MapDoc, pos: Pos2) -> Option<(u32, u32)> {
        let p = (pos - self.canvas.min - self.offset) / self.tile_px();
        let (x, y) = (p.x.floor(), p.y.floor());
        (x >= 0.0 && y >= 0.0 && x < doc.width() as f32 && y < doc.height() as f32).then_some((x as u32, y as u32))
    }

    fn tile_rect(&self, x: u32, y: u32) -> Rect {
        let t = self.tile_px();
        Rect::from_min_size(self.canvas.min + self.offset + vec2(x as f32 * t, y as f32 * t), Vec2::splat(t))
    }

    /// The status-bar tile (1-based, as placement.lua writes it) under a screen position, on the map or not.
    pub fn status_tile_at(&self, pos: Pos2) -> (i32, i32) {
        let p = (pos - self.canvas.min - self.offset) / self.tile_px();
        (p.x.floor() as i32 + 1, p.y.floor() as i32 + 1)
    }

    /// The screen rectangle of status-bar tiles x0..=x1, y0..=y1 (either corner first).
    pub fn status_rect(&self, x0: i32, y0: i32, x1: i32, y1: i32) -> Rect {
        let t = self.tile_px();
        let origin = self.canvas.min + self.offset;
        let (ax, bx) = (x0.min(x1) - 1, x0.max(x1));
        let (ay, by) = (y0.min(y1) - 1, y0.max(y1));
        Rect::from_min_max(origin + vec2(ax as f32 * t, ay as f32 * t), origin + vec2(bx as f32 * t, by as f32 * t))
    }

    /// Screen pixels per tile.
    pub fn tile_size(&self) -> f32 {
        self.tile_px()
    }

    /// The canvas's rectangle on screen as of the last frame.
    pub fn canvas(&self) -> Rect {
        self.canvas
    }

    /// Puts the map's centre in the canvas's centre.
    pub fn center(&mut self, doc: &MapDoc) {
        self.offset = (self.canvas.size() - self.map_size(doc)) / 2.0;
        self.clamp(doc);
    }

    /// Back to 100%, about the canvas's centre.
    pub fn reset_zoom(&mut self, doc: &MapDoc) {
        self.zoom_about(doc, 1.0, self.canvas.center());
    }

    /// The largest zoom step that shows the whole map.
    pub fn fit(&mut self, doc: &MapDoc) {
        let fits = |z: f32| {
            let s = vec2(doc.width() as f32, doc.height() as f32) * TILE_SIZE as f32 * z;
            s.x <= self.canvas.width() && s.y <= self.canvas.height()
        };
        self.zoom = ZOOMS.iter().rev().copied().find(|&z| fits(z)).unwrap_or(ZOOMS[0]);
        self.center(doc);
    }

    /// Changes the zoom, keeping the map point under `anchor` where it is.
    fn zoom_about(&mut self, doc: &MapDoc, zoom: f32, anchor: Pos2) {
        let local = anchor - self.canvas.min;
        let map_point = (local - self.offset) / self.zoom;
        self.zoom = zoom;
        self.offset = local - map_point * self.zoom;
        self.clamp(doc);
    }

    fn step_zoom(&mut self, doc: &MapDoc, steps: i32, anchor: Pos2) {
        let current = ZOOMS
            .iter()
            .enumerate()
            .min_by(|a, b| (a.1 - self.zoom).abs().total_cmp(&(b.1 - self.zoom).abs()))
            .map_or(5, |(i, _)| i) as i32;
        let next = (current + steps).clamp(0, ZOOMS.len() as i32 - 1) as usize;
        self.zoom_about(doc, ZOOMS[next], anchor);
    }

    /// Keeps the map on screen: a smaller map stays wholly inside, a bigger one may leave `PAN_MARGIN` of gray.
    fn clamp(&mut self, doc: &MapDoc) {
        let size = self.map_size(doc);
        let canvas = self.canvas.size();
        for (o, s, c) in [(&mut self.offset.x, size.x, canvas.x), (&mut self.offset.y, size.y, canvas.y)] {
            *o = if s <= c { o.clamp(0.0, c - s) } else { o.clamp(c - s - PAN_MARGIN, PAN_MARGIN) };
        }
    }

    /// Draws the map and handles panning and zooming. Returns the canvas's response, for the editing tools.
    pub fn show(&mut self, ui: &mut egui::Ui, doc: &mut MapDoc, selected: Option<(u32, u32)>) -> (CanvasEvents, egui::Response) {
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        self.canvas = rect;
        if self.center_pending && rect.width() > 1.0 {
            self.center_pending = false;
            self.fit(doc);
        }

        // Pan with the right or middle button; zoom with the wheel.
        for button in [PointerButton::Secondary, PointerButton::Middle] {
            if response.dragged_by(button) {
                self.offset += response.drag_delta();
                self.clamp(doc);
            }
        }
        if response.hovered() {
            // One zoom step per wheel event, whatever its size (a notch, or a burst from a touchpad).
            let (scroll, pointer) = ui.input(|i| {
                let wheel: f32 = i
                    .events
                    .iter()
                    .filter_map(|e| match e {
                        egui::Event::MouseWheel { delta, .. } => Some(delta.y.signum()),
                        _ => None,
                    })
                    .sum();
                (wheel, i.pointer.hover_pos())
            });
            if scroll != 0.0 {
                self.step_zoom(doc, scroll as i32, pointer.unwrap_or(rect.center()));
            }
        }
        if response.dragged_by(PointerButton::Secondary) || response.dragged_by(PointerButton::Middle) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        }

        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, Color32::from_gray(64));

        let map_rect = Rect::from_min_size(rect.min + self.offset, self.map_size(doc));
        let full_uv = Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0));
        let t = self.tile_px();

        // The tiles that are on screen.
        let vis = rect.intersect(map_rect);
        let x0 = (((vis.min.x - map_rect.min.x) / t).floor().max(0.0)) as u32;
        let y0 = (((vis.min.y - map_rect.min.y) / t).floor().max(0.0)) as u32;
        let x1 = (((vis.max.x - map_rect.min.x) / t).ceil() as u32).min(doc.width());
        let y1 = (((vis.max.y - map_rect.min.y) / t).ceil() as u32).min(doc.height());

        let draw_tiles = matches!(self.mode, RenderMode::GameMap | RenderMode::Passability | RenderMode::PassabilityNamed);
        if draw_tiles {
            if t < OVERVIEW_BELOW_PX {
                painter.image(doc.overview.id(), map_rect, full_uv, Color32::WHITE);
            } else {
                // One mesh per tileset texture.
                let mut meshes: Vec<Mesh> = Vec::new();
                for y in y0..y1 {
                    for x in x0..x1 {
                        let r = self.tile_rect(x, y);
                        match doc.tile_texture(x, y) {
                            Some((tex, uv)) => {
                                let id = tex.id();
                                let mesh = match meshes.iter_mut().position(|m| m.texture_id == id) {
                                    Some(i) => &mut meshes[i],
                                    None => {
                                        meshes.push(Mesh::with_texture(id));
                                        meshes.last_mut().unwrap()
                                    }
                                };
                                mesh.add_rect_with_uv(r, uv, Color32::WHITE);
                            }
                            None => {
                                painter.rect_filled(r, 0.0, Color32::from_rgb(255, 0, 255));
                            }
                        }
                    }
                }
                for mesh in meshes {
                    painter.add(mesh);
                }
            }
        }
        if let Some(tex) = doc.cell_texture(ui.ctx(), self.mode) {
            painter.image(tex.id(), map_rect, full_uv, Color32::WHITE);
        }

        if self.mode.named() && t >= NAMES_FROM_PX {
            let font = FontId::proportional((t / 4.0).clamp(9.0, 14.0));
            let over_tiles = self.mode == RenderMode::PassabilityNamed;
            for y in y0..y1 {
                for x in x0..x1 {
                    let ct = doc.tile(x, y).cell_type;
                    let at = self.tile_rect(x, y).center();
                    let name = celltype::short_name(ct);
                    if over_tiles {
                        // Over the tiles' own pixels: white with a shadow reads on any of them.
                        painter.text(at + vec2(1.0, 1.0), Align2::CENTER_CENTER, name, font.clone(), Color32::BLACK);
                        painter.text(at, Align2::CENTER_CENTER, name, font.clone(), Color32::WHITE);
                    } else {
                        let bg = self.mode.cell_color(ct).unwrap_or_default();
                        painter.text(at, Align2::CENTER_CENTER, name, font.clone(), celltype::text_color_on(bg));
                    }
                }
            }
        }

        if self.show_grid && t >= GRID_FROM_PX {
            let stroke = Stroke::new(1.0, Color32::from_black_alpha(90));
            for x in x0..=x1 {
                let sx = map_rect.min.x + x as f32 * t;
                painter.line_segment([pos2(sx, vis.min.y), pos2(sx, vis.max.y)], stroke);
            }
            for y in y0..=y1 {
                let sy = map_rect.min.y + y as f32 * t;
                painter.line_segment([pos2(vis.min.x, sy), pos2(vis.max.x, sy)], stroke);
            }
        }

        let mut events = CanvasEvents::default();
        if let Some(pos) = response.hover_pos() {
            events.hovered = self.tile_at(doc, pos);
        }
        if let Some((x, y)) = events.hovered {
            painter.rect_stroke(self.tile_rect(x, y), 0.0, Stroke::new(1.0, Color32::WHITE), StrokeKind::Inside);
        }
        if let Some((x, y)) = selected {
            painter.rect_stroke(self.tile_rect(x, y), 0.0, Stroke::new(2.0, Color32::YELLOW), StrokeKind::Inside);
        }
        if response.clicked_by(PointerButton::Primary) {
            events.clicked = response.interact_pointer_pos().and_then(|p| self.tile_at(doc, p));
        }
        (events, response)
    }
}
