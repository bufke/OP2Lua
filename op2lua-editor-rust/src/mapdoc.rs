//! A loaded map: the parsed `.map`, its tilesets packed into GPU textures, and the textures the far-out and cell
//! type views draw.

use std::collections::HashMap;

use eframe::egui::{self, ColorImage, Rect, TextureHandle, TextureOptions, pos2};
use op2_formats::render::{self, MapTilesets};
use op2_formats::{GameFiles, Map, TILE_SIZE, Tile};

use crate::celltype::RenderMode;

/// Tiles per row in a tileset atlas: 64 x 32 px = 2048 px wide, so 4,096 tiles fit in a 2048 x 2048 texture.
const ATLAS_COLS: u32 = 64;

/// Pixels per tile in the overview texture drawn when zoomed far out.
pub const OVERVIEW_PX: u32 = 4;

/// One tileset as a texture: its tiles in rows of `ATLAS_COLS`.
pub struct Atlas {
    /// The atlas on the GPU.
    pub texture: TextureHandle,
    size: [u32; 2],
    tile_count: u32,
}

impl Atlas {
    /// The texture coordinates of tile `graphic`, or None past the tileset's end.
    pub fn uv(&self, graphic: u32) -> Option<Rect> {
        if graphic >= self.tile_count {
            return None;
        }
        let (col, row) = (graphic % ATLAS_COLS, graphic / ATLAS_COLS);
        let (w, h) = (self.size[0] as f32, self.size[1] as f32);
        let t = TILE_SIZE as f32;
        Some(Rect::from_min_max(
            pos2(col as f32 * t / w, row as f32 * t / h),
            pos2((col + 1) as f32 * t / w, (row + 1) as f32 * t / h),
        ))
    }
}

/// The open map, ready to draw.
pub struct MapDoc {
    /// The file name, e.g. `eden01.map`.
    pub name: String,
    /// Where it was read from (a path, or `maps.vol:eden01.map`).
    pub source: String,
    /// The parsed map, and its tilesets as read (for the image export).
    pub map: Map,
    pub tilesets: MapTilesets,
    /// Indexed like `map.tilesets`; None for an empty slot or one that failed to load.
    pub atlases: Vec<Option<Atlas>>,
    /// Tilesets the map names that couldn't be read, with why.
    pub tileset_errors: Vec<String>,
    /// Tile groups whose stored name holds control characters (stale bytes some maps carry; the map still works).
    pub corrupt_group_names: usize,
    /// The whole map at `OVERVIEW_PX` per tile.
    pub overview: TextureHandle,
    /// One pixel per tile in each cell type view's colours, made the first time the view is used.
    cell_textures: HashMap<RenderMode, TextureHandle>,
}

impl MapDoc {
    /// Reads `name` (a bare map name found in the game's files, or a path) and builds its textures.
    pub fn load(ctx: &egui::Context, files: &GameFiles, name: &str) -> Result<Self, String> {
        let (data, source) = files.read(name).map_err(|e| e.to_string())?;
        let map = Map::parse(&data).map_err(|e| format!("{source}: {e}"))?;
        let tilesets = MapTilesets::load(files, &map);

        let mut atlases = Vec::new();
        let mut tileset_errors = Vec::new();
        for (i, slot) in tilesets.sets.iter().enumerate() {
            let tileset_name = map.tileset_name(i as u16).unwrap_or("?").to_string();
            match slot {
                None => atlases.push(None),
                Some(Err(e)) => {
                    tileset_errors.push(format!("{tileset_name}: {e}"));
                    atlases.push(None);
                }
                Some(Ok(ts)) => atlases.push(Some(build_atlas(ctx, &tileset_name, ts))),
            }
        }

        let img = render::compose(&map, &tilesets, OVERVIEW_PX);
        let overview = ctx.load_texture(
            "overview",
            ColorImage::from_rgba_unmultiplied([img.width as usize, img.height as usize], &img.rgba),
            TextureOptions::LINEAR,
        );

        let file_name = std::path::Path::new(name)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| name.to_string());
        let corrupt_group_names =
            map.tile_groups.iter().filter(|g| g.name_bytes.iter().any(|&b| b != 0 && b < 32)).count();
        Ok(Self {
            name: file_name,
            source: source.to_string(),
            map,
            tilesets,
            atlases,
            tileset_errors,
            corrupt_group_names,
            overview,
            cell_textures: HashMap::new(),
        })
    }

    /// The map's size in tiles.
    pub fn width(&self) -> u32 {
        self.map.width
    }

    pub fn height(&self) -> u32 {
        self.map.height
    }

    /// The tile at 0-based file coordinates (x, y).
    pub fn tile(&self, x: u32, y: u32) -> Tile {
        self.map.tile(x, y)
    }

    /// The texture and texture coordinates the tile at (x, y) draws with (first animation frame).
    pub fn tile_texture(&self, x: u32, y: u32) -> Option<(&TextureHandle, Rect)> {
        let m = self.map.mapping(self.map.tile(x, y))?;
        let atlas = self.atlases.get(m.tileset as usize)?.as_ref()?;
        Some((&atlas.texture, atlas.uv(m.graphic as u32)?))
    }

    /// The one-pixel-per-tile texture of a cell type view.
    pub fn cell_texture(&mut self, ctx: &egui::Context, mode: RenderMode) -> Option<&TextureHandle> {
        if !self.cell_textures.contains_key(&mode) {
            mode.cell_color(op2_formats::CellType::Rubble)?;
            let (w, h) = (self.map.width as usize, self.map.height as usize);
            let mut pixels = Vec::with_capacity(w * h);
            for y in 0..h as u32 {
                for x in 0..w as u32 {
                    pixels.push(mode.cell_color(self.map.tile(x, y).cell_type).unwrap_or_default());
                }
            }
            let image = ColorImage { size: [w, h], source_size: egui::vec2(w as f32, h as f32), pixels };
            let tex = ctx.load_texture(format!("cells-{mode:?}"), image, TextureOptions::NEAREST);
            self.cell_textures.insert(mode, tex);
        }
        self.cell_textures.get(&mode)
    }
}

fn build_atlas(ctx: &egui::Context, name: &str, ts: &op2_formats::Tileset) -> Atlas {
    let n = ts.tile_count.max(1);
    let cols = ATLAS_COLS.min(n);
    let rows = n.div_ceil(cols);
    let (w, h) = (cols * TILE_SIZE, rows * TILE_SIZE);
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    let row_bytes = (TILE_SIZE * 4) as usize;
    for i in 0..ts.tile_count {
        let Some(tile) = ts.tile(i) else { continue };
        let (cx, cy) = (i % cols, i / cols);
        for r in 0..TILE_SIZE {
            let dst = (((cy * TILE_SIZE + r) * w + cx * TILE_SIZE) * 4) as usize;
            tile.write_row(r, &mut rgba[dst..dst + row_bytes]);
        }
    }
    let texture = ctx.load_texture(
        format!("tileset-{name}"),
        ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba),
        TextureOptions::NEAREST,
    );
    Atlas { texture, size: [w, h], tile_count: ts.tile_count }
}
