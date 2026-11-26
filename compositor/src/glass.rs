//! P3-T05 glass material pipeline and effect tiers (design doc D1, section 4).
//!
//! Pure data model plus a cost estimate: no GL/DRM, so it unit-tests on the
//! macOS dev host and in CI. The values are the documented ones from
//! `docs/2026/11/16/os-project/design/ui-design.md` section 4:
//!
//! | Layer | Full | Reduced | Minimal |
//! | --- | --- | --- | --- |
//! | Backdrop blur | Dual Kawase, radius 24 px | Dual Kawase at half resolution, radius 16 px | none |
//! | Saturation | 1.2x | 1.1x | 1.0x |
//! | Tint | 18% white (light) / 18% black (dark) | same | solid 92% opacity |
//! | Edge highlight | 1 px, 40% white (brighter on top) | 1 px, 30% | 1 px, 20% |
//! | Edge refraction | on | off | off |
//! | Shadow | 12 px, 12% black | 8 px, 10% | 4 px, 10% |
//! | Corner radius | 18 px panels, 12 px menus, 8 px buttons | same | same |
//!
//! `AetherCompositor` owns a [`GlassPipeline`]: it resolves the plan for the
//! visible windows on every frame that has damage (blur only over the damaged
//! region, per the design rule "the compositor applies blur only to visible
//! regions (damage-based)"), prints `AETHER_COMPOSITOR_GLASS_PLAN`, and can
//! switch tiers at runtime (`set_effect_tier`, `AETHER_COMPOSITOR_EFFECT_TIER`).
//!
//! **Hardware gap:** [`GpuProfile::reference`] is a *modelled* profile (pass
//! throughput constants, `measured: false`). It answers "does the Full-tier
//! plan fit the 16.67 ms frame budget" analytically; real GPU validation with
//! three windows on the Dell is still pending the P2 graphics stack.

use crate::scheduling::union_area;
use crate::wm::Rect;

/// Frame rate the tier policy and the cost model budget against.
pub const DEFAULT_TARGET_FPS: u32 = 60;

/// Band width (logical px) along the surface edge that the refraction shader
/// displaces (design doc: "subtle displacement of backdrop at edges").
pub const REFRACTION_BAND_PX: f64 = 24.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlassTier {
    Full,
    Reduced,
    Minimal,
}

impl GlassTier {
    pub fn name(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Reduced => "reduced",
            Self::Minimal => "minimal",
        }
    }

    /// Parse a tier name (case-insensitive); `None` for unknown values so the
    /// caller can fall back to the default instead of failing.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "full" => Some(Self::Full),
            "reduced" => Some(Self::Reduced),
            "minimal" => Some(Self::Minimal),
            _ => None,
        }
    }

    /// Rank for ordering/tests: Full = 2, Reduced = 1, Minimal = 0.
    pub fn rank(self) -> u8 {
        match self {
            Self::Full => 2,
            Self::Reduced => 1,
            Self::Minimal => 0,
        }
    }

    pub fn lower(self) -> Option<Self> {
        match self {
            Self::Full => Some(Self::Reduced),
            Self::Reduced => Some(Self::Minimal),
            Self::Minimal => None,
        }
    }

    pub fn raise(self) -> Option<Self> {
        match self {
            Self::Full => None,
            Self::Reduced => Some(Self::Full),
            Self::Minimal => Some(Self::Reduced),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RadiusClass {
    Panel,
    Menu,
    Button,
}

impl RadiusClass {
    pub fn radius_px(self) -> i32 {
        match self {
            Self::Panel => 18,
            Self::Menu => 12,
            Self::Button => 8,
        }
    }
}

/// Dual Kawase backdrop blur: `dual_kawase_levels` downsample + upsample
/// passes, each level at a quarter of the previous area.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlurSpec {
    pub dual_kawase_levels: u8,
    pub radius_px: f32,
    /// 1 = full resolution, 2 = half resolution (Reduced tier).
    pub resolution_divisor: u8,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tint {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub alpha: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EdgeHighlight {
    pub width_px: f32,
    pub alpha: f32,
    /// Extra alpha on the top edge (the glass "catches the light").
    pub top_boost: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shadow {
    pub blur_px: f32,
    pub alpha: f32,
}

/// One resolved glass surface: the effect plan is computed per surface.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlassMaterial {
    pub tier: GlassTier,
    pub blur: Option<BlurSpec>,
    pub saturation: f32,
    pub tint: Tint,
    pub edge: EdgeHighlight,
    pub edge_refraction: bool,
    pub shadow: Shadow,
    pub corner_radius: i32,
}

/// A shader/composite stage in the glass pipeline, in back-to-front order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PassKind {
    Shadow,
    Downsample,
    Upsample,
    Saturation,
    Tint,
    EdgeRefraction,
    EdgeHighlight,
    Composite,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pass {
    pub kind: PassKind,
    pub resolution_divisor: u8,
    pub radius_px: f32,
}

impl GlassMaterial {
    /// Resolve the material for one tier (light theme).
    pub fn for_tier(tier: GlassTier) -> Self {
        Self::for_tier_theme(tier, Theme::Light)
    }

    pub fn for_tier_theme(tier: GlassTier, theme: Theme) -> Self {
        let full = Self {
            tier: GlassTier::Full,
            blur: Some(BlurSpec {
                dual_kawase_levels: 5,
                radius_px: 24.0,
                resolution_divisor: 1,
            }),
            saturation: 1.2,
            tint: tint_for(theme, 0.18),
            edge: EdgeHighlight {
                width_px: 1.0,
                alpha: 0.40,
                top_boost: 0.20,
            },
            edge_refraction: true,
            shadow: Shadow {
                blur_px: 12.0,
                alpha: 0.12,
            },
            corner_radius: RadiusClass::Panel.radius_px(),
        };
        let reduced = Self {
            tier: GlassTier::Reduced,
            blur: Some(BlurSpec {
                dual_kawase_levels: 4,
                radius_px: 16.0,
                resolution_divisor: 2,
            }),
            saturation: 1.1,
            tint: full.tint,
            edge: EdgeHighlight {
                width_px: 1.0,
                alpha: 0.30,
                top_boost: 0.10,
            },
            edge_refraction: false,
            shadow: Shadow {
                blur_px: 8.0,
                alpha: 0.10,
            },
            corner_radius: RadiusClass::Panel.radius_px(),
        };
        let minimal = Self {
            tier: GlassTier::Minimal,
            blur: None,
            saturation: 1.0,
            // Solid 92% opacity panel; no backdrop shows through.
            tint: Tint {
                r: 0.97,
                g: 0.97,
                b: 0.98,
                alpha: 0.92,
            },
            edge: EdgeHighlight {
                width_px: 1.0,
                alpha: 0.20,
                top_boost: 0.0,
            },
            edge_refraction: false,
            shadow: Shadow {
                blur_px: 4.0,
                alpha: 0.10,
            },
            corner_radius: RadiusClass::Panel.radius_px(),
        };
        match tier {
            GlassTier::Full => full,
            GlassTier::Reduced => reduced,
            GlassTier::Minimal => minimal,
        }
    }

    pub fn with_corner_radius(mut self, radius_class: RadiusClass) -> Self {
        self.corner_radius = radius_class.radius_px();
        self
    }

    /// Resolved pass list for the tier, in back-to-front order.
    pub fn passes(&self) -> Vec<Pass> {
        let mut passes = vec![Pass {
            kind: PassKind::Shadow,
            resolution_divisor: 1,
            radius_px: self.shadow.blur_px,
        }];
        if let Some(blur) = self.blur {
            for level in 0..blur.dual_kawase_levels {
                let divisor = blur
                    .resolution_divisor
                    .saturating_mul(1u8 << level.min(7));
                passes.push(Pass {
                    kind: PassKind::Downsample,
                    resolution_divisor: divisor,
                    radius_px: blur.radius_px,
                });
            }
            for level in (0..blur.dual_kawase_levels).rev() {
                let divisor = blur
                    .resolution_divisor
                    .saturating_mul(1u8 << level.min(7));
                passes.push(Pass {
                    kind: PassKind::Upsample,
                    resolution_divisor: divisor,
                    radius_px: blur.radius_px,
                });
            }
        }
        passes.push(Pass {
            kind: PassKind::Saturation,
            resolution_divisor: 1,
            radius_px: 0.0,
        });
        passes.push(Pass {
            kind: PassKind::Tint,
            resolution_divisor: 1,
            radius_px: 0.0,
        });
        if self.edge_refraction {
            passes.push(Pass {
                kind: PassKind::EdgeRefraction,
                resolution_divisor: 1,
                radius_px: REFRACTION_BAND_PX as f32,
            });
        }
        passes.push(Pass {
            kind: PassKind::EdgeHighlight,
            resolution_divisor: 1,
            radius_px: self.edge.width_px,
        });
        passes.push(Pass {
            kind: PassKind::Composite,
            resolution_divisor: 1,
            radius_px: 0.0,
        });
        passes
    }

    /// One-line human/CI readable description used in the compositor marker.
    pub fn describe(&self) -> String {
        let blur = match self.blur {
            Some(blur) => format!(
                "dual-kawase levels={} res={}x radius={:.0}",
                blur.dual_kawase_levels, blur.resolution_divisor, blur.radius_px
            ),
            None => "none".to_string(),
        };
        format!(
            "tier={} blur={} saturation={:.2} tint={:.2} edge={:.0}px:{:.2} refraction={} shadow={:.0}px:{:.2} corner={}",
            self.tier.name(),
            blur,
            self.saturation,
            self.tint.alpha,
            self.edge.width_px,
            self.edge.alpha,
            if self.edge_refraction { "on" } else { "off" },
            self.shadow.blur_px,
            self.shadow.alpha,
            self.corner_radius
        )
    }
}

fn tint_for(theme: Theme, alpha: f32) -> Tint {
    match theme {
        Theme::Light => Tint {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            alpha,
        },
        Theme::Dark => Tint {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            alpha,
        },
    }
}

/// A glass surface the compositor composites (one per managed window).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlassSurface {
    pub id: u64,
    pub rect: Rect,
    pub corner_radius: i32,
    pub opacity: f32,
}

impl GlassSurface {
    pub fn window(id: u64, rect: Rect) -> Self {
        Self {
            id,
            rect,
            corner_radius: RadiusClass::Panel.radius_px(),
            opacity: 1.0,
        }
    }
}

/// Pass throughput in pixels per millisecond. `measured: false` means the
/// numbers are modelled from the design budget, not sampled on hardware.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GpuProfile {
    pub name: &'static str,
    pub blur_px_per_ms: f64,
    pub refraction_px_per_ms: f64,
    pub composite_px_per_ms: f64,
    pub measured: bool,
}

impl GpuProfile {
    /// Reference integrated-GPU profile used for the headless budget check.
    /// Modelled: refine with numbers from the Dell once the P2 graphics stack
    /// is validated (P3-T05 hardware gate).
    pub fn reference() -> Self {
        Self {
            name: "reference-igpu-modelled",
            blur_px_per_ms: 3_000_000.0,
            refraction_px_per_ms: 800_000.0,
            composite_px_per_ms: 20_000_000.0,
            measured: false,
        }
    }

    /// Deliberately slow profile used by tests to exercise tier downgrades.
    pub fn slow() -> Self {
        Self {
            name: "slow-test",
            blur_px_per_ms: 50_000.0,
            refraction_px_per_ms: 20_000.0,
            composite_px_per_ms: 500_000.0,
            measured: false,
        }
    }

    pub fn describe(&self) -> String {
        format!(
            "name={} measured={} blur_px_per_ms={:.0} refraction_px_per_ms={:.0} composite_px_per_ms={:.0}",
            self.name,
            self.measured,
            self.blur_px_per_ms,
            self.refraction_px_per_ms,
            self.composite_px_per_ms
        )
    }
}

/// Effect plan for one surface on one frame.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectPlan {
    pub tier: GlassTier,
    pub passes: Vec<Pass>,
    /// Union area of the damaged regions processed this frame.
    pub damaged_pixels: u64,
    /// Whole-surface area (what a full repaint would cost).
    pub covered_pixels: u64,
    pub estimated_ms: f64,
}

impl EffectPlan {
    /// Fraction of the surface that had to be reprocessed (1.0 = full repaint).
    pub fn reprocessed_ratio(&self) -> f64 {
        if self.covered_pixels == 0 {
            0.0
        } else {
            self.damaged_pixels as f64 / self.covered_pixels as f64
        }
    }
}

/// Cost of one composed frame for all visible glass surfaces.
#[derive(Debug, Clone, PartialEq)]
pub struct FrameCost {
    pub tier: GlassTier,
    pub windows: usize,
    pub damaged_pixels: u64,
    pub covered_pixels: u64,
    pub estimated_ms: f64,
    pub budget_ms: f64,
}

impl FrameCost {
    pub fn share(&self) -> f64 {
        if self.budget_ms <= 0.0 {
            0.0
        } else {
            self.estimated_ms / self.budget_ms
        }
    }

    pub fn within_budget(&self) -> bool {
        self.estimated_ms <= self.budget_ms
    }
}

/// The compositor's effect pipeline: current tier, theme and GPU profile.
#[derive(Debug, Clone)]
pub struct GlassPipeline {
    tier: GlassTier,
    theme: Theme,
    profile: GpuProfile,
    target_fps: u32,
}

impl GlassPipeline {
    pub fn new(tier: GlassTier) -> Self {
        Self {
            tier,
            theme: Theme::Light,
            profile: GpuProfile::reference(),
            target_fps: DEFAULT_TARGET_FPS,
        }
    }

    pub fn with_profile(tier: GlassTier, profile: GpuProfile) -> Self {
        Self {
            profile,
            ..Self::new(tier)
        }
    }

    pub fn with_theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    pub fn tier(&self) -> GlassTier {
        self.tier
    }

    pub fn set_tier(&mut self, tier: GlassTier) {
        self.tier = tier;
    }

    pub fn profile(&self) -> &GpuProfile {
        &self.profile
    }

    pub fn budget_ms(&self) -> f64 {
        1_000.0 / self.target_fps.max(1) as f64
    }

    pub fn material(&self) -> GlassMaterial {
        GlassMaterial::for_tier_theme(self.tier, self.theme)
    }

    /// Resolve the plan for one surface. `damage` is in the same coordinate
    /// space as `surface.rect`; `None` (or an empty list) means a full repaint.
    pub fn plan(&self, surface: &GlassSurface, damage: Option<&[Rect]>) -> EffectPlan {
        let material = self.material();
        let covered = area(surface.rect);
        let damage_rects: Vec<Rect> = match damage {
            Some(rects) if !rects.is_empty() => rects
                .iter()
                .filter_map(|rect| clip(*rect, surface.rect))
                .collect(),
            _ => vec![surface.rect],
        };
        let damaged = union_area(&damage_rects);
        let estimated_ms = self.cost(&material, damaged, &damage_rects);
        EffectPlan {
            tier: self.tier,
            passes: material.passes(),
            damaged_pixels: damaged,
            covered_pixels: covered,
            estimated_ms,
        }
    }

    /// Cost of one frame with `(surface, damage)` pairs.
    pub fn frame_cost(&self, surfaces: &[(GlassSurface, Vec<Rect>)]) -> FrameCost {
        let mut damaged_pixels = 0;
        let mut covered_pixels = 0;
        let mut estimated_ms = 0.0;
        for (surface, damage) in surfaces {
            let plan = self.plan(surface, Some(damage));
            damaged_pixels += plan.damaged_pixels;
            covered_pixels += plan.covered_pixels;
            estimated_ms += plan.estimated_ms;
        }
        FrameCost {
            tier: self.tier,
            windows: surfaces.len(),
            damaged_pixels,
            covered_pixels,
            estimated_ms,
            budget_ms: self.budget_ms(),
        }
    }

    fn cost(&self, material: &GlassMaterial, damaged: u64, damage_rects: &[Rect]) -> f64 {
        let profile = &self.profile;
        let mut ms = 0.0;
        if let Some(blur) = material.blur {
            // Each level runs one downsample and one upsample pass over its
            // own resolution; each level is a quarter of the previous area.
            let mut level_area = damaged as f64;
            let mut work = 0.0;
            for _ in 0..blur.dual_kawase_levels {
                work += level_area * 2.0;
                level_area /= 4.0;
            }
            let divisor_sq = (blur.resolution_divisor as f64).powi(2);
            ms += work / divisor_sq / profile.blur_px_per_ms;
        }
        if material.edge_refraction {
            ms += band_pixels(damage_rects, REFRACTION_BAND_PX) / profile.refraction_px_per_ms;
        }
        // Shadow, tint, edge highlight and the final composite run over the
        // damaged region; the shadow adds a band around it.
        let mut composite = damaged as f64;
        if material.shadow.alpha > 0.0 {
            composite += band_pixels(damage_rects, material.shadow.blur_px as f64);
        }
        ms += composite / profile.composite_px_per_ms;
        ms
    }
}

/// Automatic tier selection with hysteresis (P3-T05 "tier switch").
///
/// `dwell` consecutive over-budget frames drop one tier; `4 * dwell`
/// consecutive comfortably-in-budget frames raise one tier. The asymmetry
/// prevents oscillation when the compositor sits near the budget.
#[derive(Debug, Clone)]
pub struct TierPolicy {
    pub budget_ms: f64,
    pub dwell: u32,
    over: u32,
    under: u32,
}

impl TierPolicy {
    pub fn new(target_fps: u32) -> Self {
        Self {
            budget_ms: 1_000.0 / target_fps.max(1) as f64,
            dwell: 30,
            over: 0,
            under: 0,
        }
    }

    pub fn with_dwell(mut self, dwell: u32) -> Self {
        self.dwell = dwell.max(1);
        self
    }

    /// Feed one measured frame time; returns the tier to switch to, if any.
    pub fn observe(&mut self, tier: GlassTier, frame_ms: f64) -> Option<GlassTier> {
        if frame_ms > self.budget_ms {
            self.under = 0;
            self.over += 1;
            if self.over >= self.dwell {
                self.over = 0;
                return tier.lower();
            }
        } else if frame_ms < self.budget_ms * 0.6 {
            self.over = 0;
            self.under += 1;
            if self.under >= self.dwell.saturating_mul(4) {
                self.under = 0;
                return tier.raise();
            }
        } else {
            self.over = 0;
            self.under = 0;
        }
        None
    }
}

fn area(rect: Rect) -> u64 {
    if rect.w <= 0 || rect.h <= 0 {
        0
    } else {
        rect.w as u64 * rect.h as u64
    }
}

/// Intersection of two rects, or `None` when they do not overlap.
fn clip(rect: Rect, bounds: Rect) -> Option<Rect> {
    let x0 = rect.x.max(bounds.x);
    let y0 = rect.y.max(bounds.y);
    let x1 = (rect.x + rect.w).min(bounds.x + bounds.w);
    let y1 = (rect.y + rect.h).min(bounds.y + bounds.h);
    if x1 <= x0 || y1 <= y0 {
        None
    } else {
        Some(Rect {
            x: x0,
            y: y0,
            w: x1 - x0,
            h: y1 - y0,
        })
    }
}

/// Approximate ring area around the damaged rects (used for the refraction
/// band and the drop shadow): perimeter x band + corners.
fn band_pixels(rects: &[Rect], band: f64) -> f64 {
    rects
        .iter()
        .filter(|rect| rect.w > 0 && rect.h > 0)
        .map(|rect| {
            let perimeter = 2.0 * (rect.w as f64 + rect.h as f64);
            perimeter * band + 4.0 * band * band
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(id: u64, x: i32, y: i32, w: i32, h: i32) -> GlassSurface {
        GlassSurface::window(id, Rect { x, y, w, h })
    }

    #[test]
    fn tier_names_round_trip() {
        for tier in [GlassTier::Full, GlassTier::Reduced, GlassTier::Minimal] {
            assert_eq!(GlassTier::parse(tier.name()), Some(tier));
        }
        assert_eq!(GlassTier::parse("  REDUCED "), Some(GlassTier::Reduced));
        assert_eq!(GlassTier::parse("ultra"), None);
        assert_eq!(GlassTier::Full.rank(), 2);
        assert_eq!(GlassTier::Minimal.rank(), 0);
        assert_eq!(GlassTier::Full.lower(), Some(GlassTier::Reduced));
        assert_eq!(GlassTier::Minimal.lower(), None);
        assert_eq!(GlassTier::Minimal.raise(), Some(GlassTier::Reduced));
        assert_eq!(GlassTier::Full.raise(), None);
    }

    #[test]
    fn full_tier_matches_the_design_table() {
        let material = GlassMaterial::for_tier(GlassTier::Full);
        let blur = material.blur.expect("full tier blurs the backdrop");
        assert_eq!(blur.radius_px, 24.0);
        assert_eq!(blur.resolution_divisor, 1);
        assert_eq!(blur.dual_kawase_levels, 5);
        assert_eq!(material.saturation, 1.2);
        assert!((material.tint.alpha - 0.18).abs() < f32::EPSILON);
        assert!(material.tint.r == 1.0, "light theme tints white");
        assert_eq!(material.edge.alpha, 0.40);
        assert!(material.edge.top_boost > 0.0, "top edge is brighter");
        assert!(material.edge_refraction, "refraction is Full-tier only");
        assert_eq!(material.shadow.blur_px, 12.0);
        assert_eq!(material.shadow.alpha, 0.12);
        assert_eq!(material.corner_radius, 18);
    }

    #[test]
    fn reduced_tier_blurs_at_half_resolution_without_refraction() {
        let material = GlassMaterial::for_tier(GlassTier::Reduced);
        let blur = material.blur.expect("reduced tier still blurs");
        assert_eq!(blur.radius_px, 16.0);
        assert_eq!(blur.resolution_divisor, 2);
        assert_eq!(material.saturation, 1.1);
        assert_eq!(material.edge.alpha, 0.30);
        assert!(!material.edge_refraction);
        assert_eq!(material.shadow.blur_px, 8.0);
    }

    #[test]
    fn minimal_tier_is_a_solid_panel() {
        let material = GlassMaterial::for_tier(GlassTier::Minimal);
        assert!(material.blur.is_none(), "minimal tier has no backdrop blur");
        assert_eq!(material.saturation, 1.0);
        assert!((material.tint.alpha - 0.92).abs() < f32::EPSILON);
        assert_eq!(material.edge.alpha, 0.20);
        assert!(!material.edge_refraction);
        assert_eq!(material.shadow.blur_px, 4.0);
        let kinds: Vec<PassKind> = material.passes().iter().map(|pass| pass.kind).collect();
        assert!(!kinds.contains(&PassKind::Downsample));
        assert!(!kinds.contains(&PassKind::Upsample));
        assert!(!kinds.contains(&PassKind::EdgeRefraction));
        assert!(kinds.contains(&PassKind::Tint));
    }

    #[test]
    fn dark_theme_tints_black() {
        let material =
            GlassMaterial::for_tier_theme(GlassTier::Full, Theme::Dark);
        assert_eq!(material.tint.r, 0.0);
        assert_eq!(material.tint.alpha, 0.18);
    }

    #[test]
    fn pass_count_shrinks_with_the_tier() {
        let full = GlassMaterial::for_tier(GlassTier::Full).passes().len();
        let reduced = GlassMaterial::for_tier(GlassTier::Reduced).passes().len();
        let minimal = GlassMaterial::for_tier(GlassTier::Minimal).passes().len();
        assert!(full > reduced, "full={full} reduced={reduced}");
        assert!(reduced > minimal, "reduced={reduced} minimal={minimal}");
        // Full: shadow + 5 down + 5 up + saturation + tint + refraction +
        // highlight + composite.
        assert_eq!(full, 16);
        assert_eq!(minimal, 5);
    }

    #[test]
    fn blur_only_processes_the_damaged_region() {
        let pipeline = GlassPipeline::new(GlassTier::Full);
        let surface = window(1, 0, 0, 1000, 800);
        let damage = [Rect { x: 0, y: 0, w: 1000, h: 100 }];
        let plan = pipeline.plan(&surface, Some(&damage));
        assert_eq!(plan.covered_pixels, 1000 * 800);
        assert_eq!(plan.damaged_pixels, 1000 * 100);
        assert!((plan.reprocessed_ratio() - 0.125).abs() < 1e-9);

        let full_plan = pipeline.plan(&surface, None);
        assert!(
            full_plan.estimated_ms > plan.estimated_ms * 4.0,
            "full repaint is much more expensive than a strip: {} vs {}",
            full_plan.estimated_ms,
            plan.estimated_ms
        );
    }

    #[test]
    fn damage_outside_the_surface_is_clipped() {
        let pipeline = GlassPipeline::new(GlassTier::Full);
        let surface = window(1, 100, 100, 200, 200);
        let damage = [Rect { x: 0, y: 0, w: 400, h: 400 }];
        let plan = pipeline.plan(&surface, Some(&damage));
        assert_eq!(plan.damaged_pixels, 200 * 200);
    }

    #[test]
    fn lower_tiers_cost_less() {
        let surface = window(1, 0, 0, 1152, 648);
        let cost = |tier| {
            GlassPipeline::new(tier)
                .plan(&surface, None)
                .estimated_ms
        };
        let full = cost(GlassTier::Full);
        let reduced = cost(GlassTier::Reduced);
        let minimal = cost(GlassTier::Minimal);
        assert!(full > reduced, "full={full} reduced={reduced}");
        assert!(reduced > minimal, "reduced={reduced} minimal={minimal}");
    }

    #[test]
    fn three_windows_at_full_tier_fit_the_60fps_budget() {
        // P3-T05 acceptance, headless half: three 1152x648 windows (60% of the
        // 1920x1080 primary output) composited at Full tier on the modelled
        // reference GPU must fit one 60 Hz frame. Real GPU numbers are pending
        // the Dell (see the module docs).
        let pipeline = GlassPipeline::new(GlassTier::Full);
        let surfaces: Vec<(GlassSurface, Vec<Rect>)> = (0..3)
            .map(|index| {
                let x = 128 + index * 160;
                let y = 144 + index * 96;
                let surface = window(index as u64, x, y, 1152, 648);
                (surface, vec![surface.rect])
            })
            .collect();
        let cost = pipeline.frame_cost(&surfaces);
        assert_eq!(cost.windows, 3);
        assert_eq!(cost.damaged_pixels, 3 * 1152 * 648);
        assert!(
            cost.within_budget(),
            "full tier frame cost {:.2} ms exceeds the {:.2} ms budget",
            cost.estimated_ms,
            cost.budget_ms
        );
        assert!(
            cost.share() < 0.5,
            "estimated Full-tier frame uses {:.0}% of the budget",
            cost.share() * 100.0
        );
        assert!(!pipeline.profile().measured, "profile is modelled, not measured");
    }

    #[test]
    fn slow_gpu_pushes_full_tier_over_budget() {
        let pipeline = GlassPipeline::with_profile(GlassTier::Full, GpuProfile::slow());
        let surface = window(1, 0, 0, 1152, 648);
        let cost = pipeline.frame_cost(&[(surface, vec![surface.rect])]);
        assert!(!cost.within_budget());
    }

    #[test]
    fn tier_policy_drops_after_dwell_over_budget() {
        let mut policy = TierPolicy::new(60).with_dwell(3);
        assert_eq!(policy.observe(GlassTier::Full, 18.0), None);
        assert_eq!(policy.observe(GlassTier::Full, 18.0), None);
        assert_eq!(
            policy.observe(GlassTier::Full, 18.0),
            Some(GlassTier::Reduced)
        );
        // Reduced over budget again drops to Minimal; Minimal stays.
        for _ in 0..3 {
            policy.observe(GlassTier::Reduced, 20.0);
        }
        assert_eq!(policy.observe(GlassTier::Reduced, 20.0), None);
        for _ in 0..3 {
            policy.observe(GlassTier::Minimal, 20.0);
        }
        assert_eq!(policy.observe(GlassTier::Minimal, 20.0), None);
    }

    #[test]
    fn tier_policy_raises_only_after_a_long_comfortable_run() {
        let mut policy = TierPolicy::new(60).with_dwell(2);
        // 8 comfortable frames (4 x dwell) raise one tier.
        for _ in 0..7 {
            assert_eq!(policy.observe(GlassTier::Minimal, 4.0), None);
        }
        assert_eq!(
            policy.observe(GlassTier::Minimal, 4.0),
            Some(GlassTier::Reduced)
        );
        // A frame near the budget resets the counters instead of raising.
        policy.observe(GlassTier::Reduced, 12.0);
        for _ in 0..7 {
            assert_eq!(policy.observe(GlassTier::Reduced, 4.0), None);
        }
    }

    #[test]
    fn describe_is_single_line_and_names_the_tier() {
        let text = GlassMaterial::for_tier(GlassTier::Reduced).describe();
        assert!(text.starts_with("tier=reduced "), "{text}");
        assert!(text.contains("blur=dual-kawase levels=4 res=2x radius=16"), "{text}");
        assert!(text.contains("refraction=off"), "{text}");
        assert!(!text.contains('\n'));
    }
}
