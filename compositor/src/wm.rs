//! P3-T04 window management skeleton: floating, snap, tiling, workspaces.
//!
//! Pure data model (no Wayland/DRM) so it unit-tests on the dev host and in CI.
//! `AetherCompositor` in `lib.rs` owns a `WorkspaceManager`, registers each
//! xdg-toplevel as a floating window on the active workspace, and pushes the
//! managed geometry back to clients (`snap_toplevel`, `tile_active_workspace`,
//! `activate_workspace`). Layer surfaces are shell chrome and are not managed.

use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WindowId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WorkspaceId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapEdge {
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowMode {
    Floating,
    Tiled,
    Snapped(SnapEdge),
    Fullscreen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

#[derive(Debug, Clone)]
pub struct ManagedWindow {
    pub id: WindowId,
    pub workspace: WorkspaceId,
    pub mode: WindowMode,
    pub rect: Rect,
}

#[derive(Debug)]
pub struct WorkspaceManager {
    pub workspaces: Vec<WorkspaceId>,
    pub active: WorkspaceId,
    pub windows: HashMap<WindowId, ManagedWindow>,
    pub output_size: (i32, i32),
}

impl WorkspaceManager {
    pub fn new(output_w: i32, output_h: i32) -> Self {
        let ws = WorkspaceId(0);
        Self {
            workspaces: vec![ws],
            active: ws,
            windows: HashMap::new(),
            output_size: (output_w, output_h),
        }
    }

    pub fn add_workspace(&mut self) -> WorkspaceId {
        let id = WorkspaceId(self.workspaces.len() as u32);
        self.workspaces.push(id);
        id
    }

    pub fn add_window(&mut self, id: WindowId, workspace: Option<WorkspaceId>) {
        let ws = workspace.unwrap_or(self.active);
        if !self.workspaces.contains(&ws) {
            self.workspaces.push(ws);
        }
        self.windows.insert(
            id,
            ManagedWindow {
                id,
                workspace: ws,
                mode: WindowMode::Floating,
                rect: Rect { x: 40, y: 40, w: 400, h: 300 },
            },
        );
    }

    pub fn move_to_workspace(&mut self, id: WindowId, ws: WorkspaceId) -> bool {
        if let Some(w) = self.windows.get_mut(&id) {
            w.workspace = ws;
            true
        } else {
            false
        }
    }

    pub fn set_mode(&mut self, id: WindowId, mode: WindowMode) {
        if let Some(w) = self.windows.get_mut(&id) {
            w.mode = mode;
            match mode {
                WindowMode::Snapped(edge) => {
                    let (ow, oh) = self.output_size;
                    w.rect = match edge {
                        SnapEdge::Left => Rect { x: 0, y: 0, w: ow / 2, h: oh },
                        SnapEdge::Right => Rect { x: ow / 2, y: 0, w: ow / 2, h: oh },
                        SnapEdge::Top => Rect { x: 0, y: 0, w: ow, h: oh / 2 },
                        SnapEdge::Bottom => Rect { x: 0, y: oh / 2, w: ow, h: oh / 2 },
                    };
                }
                WindowMode::Tiled => self.apply_tile_single(id),
                WindowMode::Fullscreen => {
                    let (ow, oh) = self.output_size;
                    w.rect = Rect { x: 0, y: 0, w: ow, h: oh };
                }
                WindowMode::Floating => {}
            }
        }
    }

    fn apply_tile_single(&mut self, id: WindowId) {
        // Single-window tile fills the output.
        if let Some(w) = self.windows.get_mut(&id) {
            let (ow, oh) = self.output_size;
            w.rect = Rect { x: 0, y: 0, w: ow, h: oh };
        }
    }

    pub fn tile_all(&mut self, workspace: WorkspaceId) {
        let ids: Vec<WindowId> = self
            .windows
            .values()
            .filter(|w| w.workspace == workspace)
            .map(|w| w.id)
            .collect();
        let n = ids.len() as i32;
        if n == 0 {
            return;
        }
        let (ow, oh) = self.output_size;
        let cols = (n as f64).sqrt().ceil() as i32;
        let rows = (n + cols - 1) / cols;
        let cw = ow / cols;
        let ch = oh / rows;
        for (i, id) in ids.iter().enumerate() {
            let i = i as i32;
            let col = i % cols;
            let row = i / cols;
            if let Some(w) = self.windows.get_mut(id) {
                w.mode = WindowMode::Tiled;
                w.rect = Rect {
                    x: col * cw,
                    y: row * ch,
                    w: cw,
                    h: ch,
                };
            }
        }
    }

    pub fn visible_on(&self, workspace: WorkspaceId) -> Vec<WindowId> {
        self.windows
            .values()
            .filter(|w| w.workspace == workspace)
            .map(|w| w.id)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspaces_and_windows() {
        let mut wm = WorkspaceManager::new(1920, 1080);
        let w2 = wm.add_workspace();
        wm.add_window(WindowId(1), None);
        wm.add_window(WindowId(2), Some(w2));
        assert_eq!(wm.visible_on(WorkspaceId(0)), vec![WindowId(1)]);
        assert_eq!(wm.visible_on(w2), vec![WindowId(2)]);
        assert!(wm.move_to_workspace(WindowId(1), w2));
        assert_eq!(wm.visible_on(w2).len(), 2);
    }

    #[test]
    fn snap_and_tile() {
        let mut wm = WorkspaceManager::new(1000, 800);
        wm.add_window(WindowId(1), None);
        wm.add_window(WindowId(2), None);
        wm.set_mode(WindowId(1), WindowMode::Snapped(SnapEdge::Left));
        let r = wm.windows.get(&WindowId(1)).unwrap().rect;
        assert_eq!((r.w, r.h), (500, 800));
        wm.tile_all(WorkspaceId(0));
        for w in wm.windows.values() {
            assert_eq!(w.mode, WindowMode::Tiled);
        }
    }

    #[test]
    fn fullscreen() {
        let mut wm = WorkspaceManager::new(1920, 1080);
        wm.add_window(WindowId(1), None);
        wm.set_mode(WindowId(1), WindowMode::Fullscreen);
        let r = wm.windows.get(&WindowId(1)).unwrap().rect;
        assert_eq!((r.w, r.h), (1920, 1080));
    }

    #[test]
    fn grid_tiling_uses_whole_output() {
        // Four windows tile into a 2x2 grid that covers the output exactly
        // (no gaps, no overlap) since the output divides evenly.
        let mut wm = WorkspaceManager::new(1920, 1080);
        for id in 0..4 {
            wm.add_window(WindowId(id), None);
        }
        wm.tile_all(WorkspaceId(0));
        let mut covered = 0;
        for w in wm.windows.values() {
            assert_eq!(w.mode, WindowMode::Tiled);
            assert_eq!((w.rect.w, w.rect.h), (960, 540));
            covered += w.rect.w * w.rect.h;
        }
        assert_eq!(covered, 1920 * 1080);
    }

    #[test]
    fn snapping_right_half() {
        let mut wm = WorkspaceManager::new(800, 600);
        wm.add_window(WindowId(1), None);
        wm.set_mode(WindowId(1), WindowMode::Snapped(SnapEdge::Right));
        let r = wm.windows.get(&WindowId(1)).unwrap().rect;
        assert_eq!((r.x, r.y, r.w, r.h), (400, 0, 400, 600));
    }
}
