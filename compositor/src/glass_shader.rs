//! P3-T05: the GLES side of the glass material pipeline.
//!
//! [`crate::glass`] resolves a tier into an [`EffectPlan`] (pass list, damage
//! area, modelled cost) but stays pure data so it unit-tests on the macOS dev
//! host. This module turns a plan into the concrete shader program sequence the
//! DRM renderer executes, together with the uniform values each program needs.
//! It is likewise pure — no GL types — so the mapping is covered by
//! `cargo test -p aether-compositor` on any host:
//!
//! | Stage | Program | Runs at | Uniforms |
//! | --- | --- | --- | --- |
//! | Dual-Kawase down (per level) | [`ShaderProgram::KawaseDownsample`] | damage region / 2^level / divisor | `sample_step`, `alpha` |
//! | Dual-Kawase up (per level) | [`ShaderProgram::KawaseUpsample`] | same size as its down level | `sample_step`, `alpha` |
//! | Glass composite (fused) | [`ShaderProgram::Composite`] | damage region | `saturation`, `tint`, `edge`, `surface`, `region`, `refraction`, `fill`, `alpha` |
//!
//! The GLSL sources live in `src/shaders/` (`glass_downsample.frag`,
//! `glass_upsample.frag`, `glass_composite.frag`) and are compiled by
//! `backend::glass_gles` on the `drm` feature path only. `glass.rs` still lists
//! [`PassKind::Saturation`], `Tint`, `EdgeRefraction`, `EdgeHighlight` and
//! `Composite` separately for the cost model; the shader chain fuses them into
//! the single composite program, which keeps the modelled budget conservative.
//!
//! [`PassKind::Shadow`] has no program yet: the drop shadow is drawn by the
//! shell's decorated surfaces, so the DRM compositor does not synthesise one.
//!
//! The blur is *damage-based*: every stage runs at the size of the surface's
//! damaged region (the union of the frame damage clipped to the surface), so an
//! idle strip repaint costs a strip-sized blur, matching the design rule in D1
//! section 4. [`frost_region`] resolves that region for the DRM renderer and
//! grows it by `halo_px` so the chain never reads a clamped strip edge.

use crate::glass::{EffectPlan, GlassMaterial, GlassTier, PassKind, REFRACTION_BAND_PX};
use crate::wm::Rect;

/// GLSL source of the dual-Kawase downsample pass.
pub const DOWNSAMPLE_SHADER: &str = include_str!("shaders/glass_downsample.frag");
/// GLSL source of the dual-Kawase upsample pass.
pub const UPSAMPLE_SHADER: &str = include_str!("shaders/glass_upsample.frag");
/// GLSL source of the fused glass composite pass (saturation, tint, refraction,
/// edge highlight, corner mask, Minimal fill).
pub const COMPOSITE_SHADER: &str = include_str!("shaders/glass_composite.frag");

/// The value type of a shader uniform, mirroring the subset of smithay's
/// `UniformType` this pipeline uses so the mapping tests need no GL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UniformKind {
    Float,
    Vec2,
    Vec4,
}

/// A uniform value, independent of the GL binding.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShaderValue {
    Float(f32),
    Vec2(f32, f32),
    Vec4(f32, f32, f32, f32),
}

/// One named uniform with its resolved value for a stage.
#[derive(Debug, Clone, PartialEq)]
pub struct ShaderUniform {
    pub name: &'static str,
    pub value: ShaderValue,
}

/// A GLES program the glass chain runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShaderProgram {
    KawaseDownsample,
    KawaseUpsample,
    Composite,
}

impl ShaderProgram {
    /// Stable name for markers and docs.
    pub fn name(self) -> &'static str {
        match self {
            Self::KawaseDownsample => "kawase-down",
            Self::KawaseUpsample => "kawase-up",
            Self::Composite => "glass-composite",
        }
    }

    /// The GLSL ES 1.00 fragment source compiled for this program.
    pub fn source(self) -> &'static str {
        match self {
            Self::KawaseDownsample => DOWNSAMPLE_SHADER,
            Self::KawaseUpsample => UPSAMPLE_SHADER,
            Self::Composite => COMPOSITE_SHADER,
        }
    }

    /// Extra uniforms that must be registered when the program is compiled,
    /// in the order the compositor sets them.
    pub fn uniforms(self) -> &'static [(&'static str, UniformKind)] {
        match self {
            // `sample_step` is the only extra uniform; `alpha` is supplied by
            // the renderer itself.
            Self::KawaseDownsample | Self::KawaseUpsample => &[("sample_step", UniformKind::Vec2)],
            Self::Composite => &[
                ("saturation", UniformKind::Float),
                ("tint", UniformKind::Vec4),
                ("edge", UniformKind::Vec4),
                ("surface", UniformKind::Vec2),
                ("region", UniformKind::Vec4),
                ("refraction", UniformKind::Float),
                ("fill", UniformKind::Float),
            ],
        }
    }

    pub fn all() -> [Self; 3] {
        [
            Self::KawaseDownsample,
            Self::KawaseUpsample,
            Self::Composite,
        ]
    }
}

/// One stage of the chain: which program, how large its target is, and with
/// which uniform values, executed in the order of [`shader_stages`].
#[derive(Debug, Clone, PartialEq)]
pub struct ShaderStage {
    pub program: ShaderProgram,
    /// Pass kind of `glass.rs` that produced this stage.
    pub kind: PassKind,
    /// Pixel size of the stage's render target.
    pub size: (i32, i32),
    pub uniforms: Vec<ShaderUniform>,
}

/// Normalised sample offset (in texels of the source level) for one Kawase
/// step. The radius of the material is spread over the chain's levels; the
/// floor keeps a minimal 0.5 texel step so a 16 px blur still samples between
/// texels instead of degenerating to a copy.
pub fn kawase_offset(radius_px: f32, levels: u8) -> f32 {
    (radius_px / levels.max(1) as f32).max(0.5)
}

/// Per-tap offset in normalised coordinates for a source level of `size`.
fn sample_step(offset_px: f32, size: (i32, i32)) -> ShaderUniform {
    let w = size.0.max(1) as f32;
    let h = size.1.max(1) as f32;
    ShaderUniform {
        name: "sample_step",
        value: ShaderValue::Vec2(offset_px / w, offset_px / h),
    }
}

fn scaled(size: (i32, i32), divisor: u8) -> (i32, i32) {
    let divisor = divisor.max(1) as f32;
    (
        ((size.0 as f32 / divisor).round() as i32).max(1),
        ((size.1 as f32 / divisor).round() as i32).max(1),
    )
}

/// Clip the frame damage to one surface and return its bounding rectangle.
///
/// An empty damage list (or one that misses the surface) means nothing to draw
/// and yields a zero-sized rectangle; `None` means "the whole surface", which
/// the caller passes as an empty slice.
pub fn damage_region(surface: Rect, damage: &[Rect]) -> Rect {
    let mut union: Option<Rect> = None;
    for rect in damage {
        let x0 = rect.x.max(surface.x);
        let y0 = rect.y.max(surface.y);
        let x1 = (rect.x + rect.w).min(surface.x + surface.w);
        let y1 = (rect.y + rect.h).min(surface.y + surface.h);
        if x1 <= x0 || y1 <= y0 {
            continue;
        }
        let clipped = Rect {
            x: x0,
            y: y0,
            w: x1 - x0,
            h: y1 - y0,
        };
        union = Some(match union {
            None => clipped,
            Some(current) => {
                let ux0 = current.x.min(clipped.x);
                let uy0 = current.y.min(clipped.y);
                let ux1 = (current.x + current.w).max(clipped.x + clipped.w);
                let uy1 = (current.y + current.h).max(clipped.y + clipped.h);
                Rect {
                    x: ux0,
                    y: uy0,
                    w: ux1 - ux0,
                    h: uy1 - uy0,
                }
            }
        });
    }
    union.unwrap_or(Rect {
        x: surface.x,
        y: surface.y,
        w: 0,
        h: 0,
    })
}

/// Share of a surface above which a damage-scoped frost is not worth its own
/// pass setup: one full-surface pass is cheaper than scoping the chain.
pub const FULL_FROST_RATIO: f64 = 0.5;

/// Extra pixels (physical) the frost chain needs on every side of the damaged
/// strip so every blur level samples freshly written texels instead of the
/// clamped edge of the strip, plus the refraction band the composite pass reads
/// around the rim.
pub fn halo_px(material: &GlassMaterial) -> i32 {
    let blur = material.blur.map(|blur| blur.radius_px).unwrap_or(0.0);
    let refraction = if material.edge_refraction {
        REFRACTION_BAND_PX as f32
    } else {
        0.0
    };
    (blur + refraction).ceil() as i32
}

/// Region of one surface (physical px, origin at the surface's top-left) that
/// must be re-frosted for a frame.
///
/// - empty `damage` → the whole surface (first map, resize, VT switch);
/// - otherwise the damage clipped to the surface and grown by `halo` on every
///   side, clipped again to the surface;
/// - `None` when no damage reaches the surface;
/// - the whole surface once the grown region covers [`FULL_FROST_RATIO`] of it.
pub fn frost_region(surface: (i32, i32), damage: &[Rect], halo: i32) -> Option<Rect> {
    if surface.0 <= 0 || surface.1 <= 0 {
        return None;
    }
    let whole = Rect {
        x: 0,
        y: 0,
        w: surface.0,
        h: surface.1,
    };
    if damage.is_empty() {
        return Some(whole);
    }
    let clipped = damage_region(whole, damage);
    if clipped.w <= 0 || clipped.h <= 0 {
        return None;
    }
    let halo = halo.max(0);
    let x0 = (clipped.x - halo).max(0);
    let y0 = (clipped.y - halo).max(0);
    let x1 = (clipped.x + clipped.w + halo).min(whole.w);
    let y1 = (clipped.y + clipped.h + halo).min(whole.h);
    let grown = Rect {
        x: x0,
        y: y0,
        w: x1 - x0,
        h: y1 - y0,
    };
    let whole_area = whole.w as i64 * whole.h as i64;
    let grown_area = grown.w as i64 * grown.h as i64;
    if grown_area as f64 >= whole_area as f64 * FULL_FROST_RATIO {
        Some(whole)
    } else {
        Some(grown)
    }
}

/// [`frost_region`] for a logical surface rect and logical frame damage at an
/// output scale. `halo` is in physical pixels.
pub fn frost_region_scaled(surface: Rect, damage: &[Rect], scale: f64, halo: i32) -> Option<Rect> {
    if surface.w <= 0 || surface.h <= 0 {
        return None;
    }
    let physical_surface = (
        (surface.w as f64 * scale).round().max(1.0) as i32,
        (surface.h as f64 * scale).round().max(1.0) as i32,
    );
    if damage.is_empty() {
        return frost_region(physical_surface, &[], halo);
    }
    let clipped = damage_region(surface, damage);
    if clipped.w <= 0 || clipped.h <= 0 {
        return None;
    }
    let physical = physical_damage(surface, clipped, scale);
    frost_region(physical_surface, &[physical], halo)
}

/// Physical footprint of a surface-local `clipped` damage rect on an output at
/// `scale`, relative to the surface's physical origin.
///
/// Both edges are rounded once from their *output* coordinate, so the strip
/// covers exactly the texels the renderer draws for that damage. Rounding the
/// width on its own (`round(w * scale)`) is off by one physical pixel whenever
/// the edges fall on a fractional pixel (1.5x): the strip then reaches one pixel
/// past the damage, or is clipped one pixel short at the surface edge.
pub fn physical_damage(surface: Rect, clipped: Rect, scale: f64) -> Rect {
    let round = |value: i32| (value as f64 * scale).round() as i32;
    let origin_x = round(surface.x);
    let origin_y = round(surface.y);
    Rect {
        x: round(clipped.x) - origin_x,
        y: round(clipped.y) - origin_y,
        w: round(clipped.x + clipped.w) - round(clipped.x),
        h: round(clipped.y + clipped.h) - round(clipped.y),
    }
}

/// Logical rect -> physical output rect.
///
/// The capture region and the frost region must round identically or the chain
/// would sample texels the capture never wrote, so both go through here (the
/// DRM renderer's placement uses the same rounding).
pub fn physical_rect(rect: Rect, scale: f64) -> Rect {
    Rect {
        x: (rect.x as f64 * scale).round() as i32,
        y: (rect.y as f64 * scale).round() as i32,
        w: (rect.w as f64 * scale).round().max(0.0) as i32,
        h: (rect.h as f64 * scale).round().max(0.0) as i32,
    }
}

/// Intersection of two rects, `None` when they do not overlap.
pub fn intersect_rects(a: Rect, b: Rect) -> Option<Rect> {
    let x0 = a.x.max(b.x);
    let y0 = a.y.max(b.y);
    let x1 = (a.x + a.w).min(b.x + b.w);
    let y1 = (a.y + a.h).min(b.y + b.h);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    Some(Rect {
        x: x0,
        y: y0,
        w: x1 - x0,
        h: y1 - y0,
    })
}

/// Bounding box of `rects`, `None` when the list is empty.
pub fn union_rects(rects: &[Rect]) -> Option<Rect> {
    let mut union: Option<Rect> = None;
    for rect in rects {
        union = Some(match union {
            None => *rect,
            Some(current) => {
                let x0 = current.x.min(rect.x);
                let y0 = current.y.min(rect.y);
                let x1 = (current.x + current.w).max(rect.x + rect.w);
                let y1 = (current.y + current.h).max(rect.y + rect.h);
                Rect {
                    x: x0,
                    y: y0,
                    w: x1 - x0,
                    h: y1 - y0,
                }
            }
        });
    }
    union
}

/// Output-space regions the backdrop has to be (re)captured for this frame.
///
/// One region per glass surface: exactly the [frost region](frost_region_scaled)
/// the chain samples this frame, moved to output pixel coordinates and clipped
/// to the output. Capturing only these instead of the whole union of the glass
/// surfaces keeps the wallpaper pass proportional to the damage. Empty `damage`
/// means a full repaint, so every surface contributes its whole rect.
pub fn backdrop_regions(
    surfaces: &[Rect],
    damage: &[Rect],
    scale: f64,
    halo: i32,
    output_size: (i32, i32),
) -> Vec<Rect> {
    let output = Rect {
        x: 0,
        y: 0,
        w: output_size.0.max(0),
        h: output_size.1.max(0),
    };
    surfaces
        .iter()
        .filter_map(|surface| {
            let local = frost_region_scaled(*surface, damage, scale, halo)?;
            let origin = physical_rect(*surface, scale);
            intersect_rects(
                Rect {
                    x: origin.x + local.x,
                    y: origin.y + local.y,
                    w: local.w,
                    h: local.h,
                },
                output,
            )
        })
        .collect()
}

/// Scale the pixel-valued material fields for an output scale.
///
/// The shader works in physical pixels, so the 1 px rim and the design corner
/// radius are multiplied by the output scale; colours and factors are scale
/// independent.
pub fn physical_material(material: &GlassMaterial, scale: f64) -> GlassMaterial {
    if (scale - 1.0).abs() < f64::EPSILON {
        return *material;
    }
    GlassMaterial {
        edge: crate::glass::EdgeHighlight {
            width_px: (material.edge.width_px as f64 * scale) as f32,
            ..material.edge
        },
        corner_radius: (material.corner_radius as f64 * scale).round() as i32,
        ..*material
    }
}

/// Uniform values of the fused composite pass for one surface+region.
pub fn composite_uniforms(
    material: &GlassMaterial,
    surface: Rect,
    region: Rect,
) -> Vec<ShaderUniform> {
    let refraction = if material.edge_refraction {
        REFRACTION_BAND_PX as f32
    } else {
        0.0
    };
    // Minimal tier (no backdrop blur) draws a solid tinted panel instead of
    // sampling a backdrop; the shader still runs the same program.
    let fill = if material.blur.is_none() { 1.0 } else { 0.0 };
    vec![
        ShaderUniform {
            name: "saturation",
            value: ShaderValue::Float(material.saturation),
        },
        ShaderUniform {
            name: "tint",
            value: ShaderValue::Vec4(
                material.tint.r,
                material.tint.g,
                material.tint.b,
                material.tint.alpha,
            ),
        },
        ShaderUniform {
            name: "edge",
            value: ShaderValue::Vec4(
                material.edge.width_px,
                material.edge.alpha,
                material.edge.top_boost,
                material.corner_radius as f32,
            ),
        },
        ShaderUniform {
            name: "surface",
            value: ShaderValue::Vec2(surface.w.max(1) as f32, surface.h.max(1) as f32),
        },
        ShaderUniform {
            name: "region",
            value: ShaderValue::Vec4(
                (region.x - surface.x) as f32,
                (region.y - surface.y) as f32,
                region.w.max(1) as f32,
                region.h.max(1) as f32,
            ),
        },
        ShaderUniform {
            name: "refraction",
            value: ShaderValue::Float(refraction),
        },
        ShaderUniform {
            name: "fill",
            value: ShaderValue::Float(fill),
        },
    ]
}

/// Resolve the shader stages for one surface of one frame.
///
/// The stage order follows [`EffectPlan::passes`] (back to front): the Kawase
/// down chain, the Kawase up chain, then the fused composite. `region` is the
/// discarded-pixel region from [`damage_region`]; the caller must not call this
/// with an empty region.
///
/// Full tier → 5 down + 5 up + composite at the damage region size.
/// Reduced tier → 4 down + 4 up at half resolution + composite.
/// Minimal tier → a single composite stage with `fill = 1`.
pub fn shader_stages(
    material: &GlassMaterial,
    plan: &EffectPlan,
    surface: Rect,
    region: Rect,
) -> Vec<ShaderStage> {
    debug_assert!(region.w > 0 && region.h > 0, "empty damage region");
    debug_assert_eq!(plan.tier, material.tier);

    let levels = material
        .blur
        .map(|blur| blur.dual_kawase_levels)
        .unwrap_or(0);
    let offset = kawase_offset(
        material.blur.map(|blur| blur.radius_px).unwrap_or(0.0),
        levels,
    );

    let mut stages = Vec::new();
    // Size of the texture the next stage samples: the backdrop region for the
    // first down level, then the previous level for every later stage.
    let mut source_size = (region.w, region.h);
    for pass in &plan.passes {
        match pass.kind {
            PassKind::Downsample => {
                let size = scaled((region.w, region.h), pass.resolution_divisor);
                stages.push(ShaderStage {
                    program: ShaderProgram::KawaseDownsample,
                    kind: pass.kind,
                    size,
                    uniforms: vec![sample_step(offset, source_size)],
                });
                source_size = size;
            }
            PassKind::Upsample => {
                let size = scaled((region.w, region.h), pass.resolution_divisor);
                stages.push(ShaderStage {
                    program: ShaderProgram::KawaseUpsample,
                    kind: pass.kind,
                    size,
                    uniforms: vec![sample_step(offset, source_size)],
                });
                source_size = size;
            }
            // Fused into the composite stage below.
            PassKind::Saturation
            | PassKind::Tint
            | PassKind::EdgeRefraction
            | PassKind::EdgeHighlight => {}
            // Drawn by the shell's decorated surfaces, not by the compositor.
            PassKind::Shadow => {}
            PassKind::Composite => {
                stages.push(ShaderStage {
                    program: ShaderProgram::Composite,
                    kind: pass.kind,
                    size: (region.w, region.h),
                    uniforms: composite_uniforms(material, surface, region),
                });
            }
        }
    }
    stages
}

/// True when the tier draws a backdrop blur at all (Full and Reduced).
pub fn tier_blurs(tier: GlassTier) -> bool {
    !matches!(tier, GlassTier::Minimal)
}

/// Number of Kawase stages (down + up) a stage list contains.
pub fn blur_stages(stages: &[ShaderStage]) -> usize {
    stages
        .iter()
        .filter(|stage| {
            matches!(
                stage.program,
                ShaderProgram::KawaseDownsample | ShaderProgram::KawaseUpsample
            )
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::glass::{GlassPipeline, GlassSurface, REFRACTION_BAND_PX};
    use crate::wm::Rect;

    const DAMAGE: Rect = Rect {
        x: 64,
        y: 128,
        w: 512,
        h: 256,
    };

    fn surface() -> Rect {
        Rect {
            x: 0,
            y: 0,
            w: 1152,
            h: 648,
        }
    }

    /// The plan the DRM backend would resolve for one surface at `tier`.
    fn resolved(tier: GlassTier) -> (GlassMaterial, EffectPlan, Rect, Rect) {
        let pipeline = GlassPipeline::new(tier);
        let glass_surface = GlassSurface::window(1, surface());
        let plan = pipeline.plan(&glass_surface, Some(&[DAMAGE]));
        let material = pipeline.material();
        let region = damage_region(surface(), &[DAMAGE]);
        (material, plan, surface(), region)
    }

    #[test]
    fn halo_covers_the_blur_radius_and_the_refraction_band() {
        // Full: 24 px blur + 24 px refraction band.
        assert_eq!(halo_px(&GlassMaterial::for_tier(GlassTier::Full)), 48);
        // Reduced has no refraction: only the 16 px blur.
        assert_eq!(halo_px(&GlassMaterial::for_tier(GlassTier::Reduced)), 16);
        // Minimal samples no backdrop at all.
        assert_eq!(halo_px(&GlassMaterial::for_tier(GlassTier::Minimal)), 0);
    }

    #[test]
    fn frost_region_is_the_damaged_strip_plus_the_halo() {
        let strip = Rect {
            x: 100,
            y: 200,
            w: 320,
            h: 100,
        };
        // Away from the edges the halo grows every side.
        assert_eq!(
            frost_region((1152, 648), &[strip], 48),
            Some(Rect {
                x: 52,
                y: 152,
                w: 416,
                h: 196
            })
        );
        // Strip area 32 000 px → region 81 536 px, 10.9 % of the surface.
        let region = frost_region((1152, 648), &[strip], 48).unwrap();
        assert!(
            region.w * region.h * 4 < 1152 * 648,
            "region must stay under a quarter of the surface"
        );
    }

    #[test]
    fn frost_region_halo_is_clipped_to_the_surface() {
        let corner = Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 30,
        };
        assert_eq!(
            frost_region((1152, 648), &[corner], 48),
            Some(Rect {
                x: 0,
                y: 0,
                w: 88,
                h: 78
            })
        );
    }

    #[test]
    fn frost_region_falls_back_to_the_whole_surface() {
        // No damage (first map, resize, VT switch) → full repaint.
        assert_eq!(
            frost_region((1152, 648), &[], 48),
            Some(surface())
        );
        // Damage outside the surface → nothing to frost.
        assert_eq!(
            frost_region(
                (1152, 648),
                &[Rect {
                    x: 2000,
                    y: 2000,
                    w: 10,
                    h: 10
                }],
                48
            ),
            None
        );
        // Once the grown region covers half the surface, one pass wins.
        let big = Rect {
            x: 0,
            y: 0,
            w: 900,
            h: 500,
        };
        assert_eq!(frost_region((1152, 648), &[big], 48), Some(surface()));
    }

    #[test]
    fn frost_region_scaled_converts_to_physical_pixels() {
        let strip = Rect {
            x: 100,
            y: 200,
            w: 320,
            h: 100,
        };
        let at_1x = frost_region_scaled(surface(), &[strip], 1.0, 48).unwrap();
        let at_2x = frost_region_scaled(surface(), &[strip], 2.0, 48).unwrap();
        // The logical surface doubles, so the physical strip does too.
        assert_eq!(at_1x, Rect { x: 52, y: 152, w: 416, h: 196 });
        assert_eq!(at_2x, Rect { x: 152, y: 352, w: 736, h: 296 });
    }

    #[test]
    fn frost_region_scaled_never_clips_at_fractional_pixel_edges() {
        // HiDPI review: the frost strip must cover exactly the physical footprint
        // of the damage, and stay inside the surface, at 1.5x and 2x — even for
        // one-logical-pixel damage at an odd offset, where rounding the width
        // independently used to add or drop a physical pixel column.
        let halo = 0; // isolate the strip from the halo clipping
        for scale in [1.5, 2.0] {
            let cases = [
                (Rect { x: 0, y: 0, w: 1152, h: 648 }, Rect { x: 1, y: 1, w: 1, h: 1 }),
                (Rect { x: 1000, y: 0, w: 3, h: 32 }, Rect { x: 1001, y: 0, w: 1, h: 8 }),
                (Rect { x: 121, y: 64, w: 1151, h: 648 }, Rect { x: 122, y: 65, w: 3, h: 3 }),
                (Rect { x: 7, y: 3, w: 13, h: 9 }, Rect { x: 0, y: 0, w: 13, h: 9 }),
            ];
            for (surface, strip) in cases {
                let clipped = damage_region(surface, &[strip]);
                let physical = physical_damage(surface, clipped, scale);
                // 1. The strip starts and ends where the damage does, rounded
                //    from output coordinates.
                assert_eq!(
                    physical.x,
                    (clipped.x as f64 * scale).round() as i32
                        - (surface.x as f64 * scale).round() as i32,
                    "origin at {scale}x for {surface:?} / {strip:?}"
                );
                assert_eq!(
                    physical.x + physical.w,
                    ((clipped.x + clipped.w) as f64 * scale).round() as i32
                        - (surface.x as f64 * scale).round() as i32,
                    "far edge at {scale}x for {surface:?} / {strip:?}"
                );
                // 2. No rect is clipped: the strip and its surface fit in the
                //    physical surface, and the frost region agrees.
                assert!(physical.w > 0 && physical.h > 0);
                let region = frost_region_scaled(surface, &[strip], scale, halo).unwrap();
                let physical_surface = (
                    (surface.w as f64 * scale).round() as i32,
                    (surface.h as f64 * scale).round() as i32,
                );
                assert!(
                    region.x >= 0
                        && region.y >= 0
                        && region.x + region.w <= physical_surface.0
                        && region.y + region.h <= physical_surface.1,
                    "frost {region:?} must stay inside {physical_surface:?} at {scale}x"
                );
                assert_eq!(
                    region,
                    Rect {
                        x: physical.x,
                        y: physical.y,
                        w: physical.w,
                        h: physical.h
                    },
                    "the frost region must be the physical damage footprint at {scale}x"
                );
            }
        }
    }

    #[test]
    fn hidpi_capture_is_not_clipped_and_covers_the_frost() {
        // A window on the 2880x1800 (1.5x, 1920x1200 logical) output, near but
        // inside the right/bottom edges, and the same window on a 2x output.
        let cases = [
            (1.5, (2880, 1800), Rect { x: 760, y: 60, w: 1100, h: 700 }),
            (2.0, (1920, 1080), Rect { x: 40, y: 40, w: 880, h: 460 }),
        ];
        for (scale, output, surface) in cases {
            let halo = 48;
            let strip = Rect {
                x: surface.x + surface.w - 40,
                y: surface.y + surface.h - 24,
                w: 32,
                h: 16,
            };
            let captured = backdrop_regions(&[surface], &[strip], scale, halo, output);
            assert_eq!(captured.len(), 1, "one strip per frosted surface at {scale}x");
            let local = frost_region_scaled(surface, &[strip], scale, halo).unwrap();
            let origin = physical_rect(surface, scale);
            let frost = Rect {
                x: origin.x + local.x,
                y: origin.y + local.y,
                w: local.w,
                h: local.h,
            };
            // The capture is the frost region translated to output space, not a
            // clipped copy of it: the output bound must not cut it.
            assert_eq!(
                captured[0], frost,
                "capture must not be clipped by the output at {scale}x"
            );
            // And the surface itself fits inside the output.
            assert!(origin.x + origin.w <= output.0 && origin.y + origin.h <= output.1);
            // The strip is at the surface's far corner: the halo is clipped by
            // the surface, not by the surface's *physical* rounding.
            assert!(frost.x + frost.w <= origin.x + origin.w);
            assert!(frost.y + frost.h <= origin.y + origin.h);
        }
    }

    #[test]
    fn backdrop_regions_follow_the_damage_not_the_surfaces() {
        let left = Rect {
            x: 0,
            y: 0,
            w: 1152,
            h: 648,
        };
        let right = Rect {
            x: 1152,
            y: 0,
            w: 1152,
            h: 648,
        };
        let strip = Rect {
            x: 100,
            y: 200,
            w: 320,
            h: 100,
        };
        // Damage only inside the left surface: the right one needs no backdrop.
        assert_eq!(
            backdrop_regions(&[left, right], &[strip], 1.0, 48, (2304, 648)),
            vec![Rect {
                x: 52,
                y: 152,
                w: 416,
                h: 196
            }]
        );
        // Damage reaching both surfaces → one strip each.
        let regions = backdrop_regions(
            &[left, right],
            &[
                strip,
                Rect {
                    x: 1252,
                    y: 200,
                    w: 320,
                    h: 100,
                },
            ],
            1.0,
            48,
            (2304, 648),
        );
        assert_eq!(regions.len(), 2);
        assert_eq!(
            regions[1],
            Rect {
                x: 1204,
                y: 152,
                w: 416,
                h: 196
            }
        );
        // The bbox of both strips is still far smaller than the two surfaces.
        let union = union_rects(&regions).unwrap();
        assert!(union.w * union.h < 1152 * 648);
    }

    #[test]
    fn backdrop_regions_cover_whole_surfaces_without_damage() {
        let surface = surface();
        assert_eq!(
            backdrop_regions(&[surface], &[], 1.0, 48, (1152, 648)),
            vec![surface]
        );
        // And are clipped to the output.
        assert_eq!(
            backdrop_regions(
                &[Rect {
                    x: 1000,
                    y: 0,
                    w: 500,
                    h: 648,
                }],
                &[],
                1.0,
                48,
                (1152, 648)
            ),
            vec![Rect {
                x: 1000,
                y: 0,
                w: 152,
                h: 648
            }]
        );
        // Nothing visible → nothing to capture.
        assert!(backdrop_regions(
            &[Rect {
                x: 5000,
                y: 5000,
                w: 10,
                h: 10
            }],
            &[],
            1.0,
            48,
            (1152, 648)
        )
        .is_empty());
    }

    #[test]
    fn backdrop_regions_contain_the_frost_region_they_serve() {
        // The chain samples the frost region out of the captured backdrop, so
        // every frost region must be inside a capture region at any scale.
        let surface = Rect {
            x: 120,
            y: 64,
            w: 1152,
            h: 648,
        };
        let strip = Rect {
            x: 220,
            y: 264,
            w: 320,
            h: 100,
        };
        for scale in [1.0, 1.5, 2.0] {
            let halo = 48;
            let output = (4000, 3000);
            let captured = backdrop_regions(&[surface], &[strip], scale, halo, output);
            let local = frost_region_scaled(surface, &[strip], scale, halo).unwrap();
            let origin = physical_rect(surface, scale);
            let frost = Rect {
                x: origin.x + local.x,
                y: origin.y + local.y,
                w: local.w,
                h: local.h,
            };
            assert!(
                captured
                    .iter()
                    .any(|region| intersect_rects(*region, frost) == Some(frost)),
                "capture {captured:?} must contain the frost region {frost:?} at {scale}x"
            );
        }
    }

    #[test]
    fn physical_rect_scales_and_rounds_like_the_output_placement() {
        let rect = Rect {
            x: 10,
            y: 20,
            w: 100,
            h: 50,
        };
        assert_eq!(physical_rect(rect, 1.0), rect);
        // 1.5x: same rounding the DRM renderer uses for element placement.
        assert_eq!(
            physical_rect(rect, 1.5),
            Rect {
                x: 15,
                y: 30,
                w: 150,
                h: 75
            }
        );
        // Negative sizes never go negative.
        assert_eq!(
            physical_rect(
                Rect {
                    x: 0,
                    y: 0,
                    w: -4,
                    h: -4
                },
                2.0
            ),
            Rect {
                x: 0,
                y: 0,
                w: 0,
                h: 0
            }
        );
    }

    #[test]
    fn full_tier_runs_the_blur_chain_then_the_composite() {
        let (material, plan, surface, region) = resolved(GlassTier::Full);
        let chain = shader_stages(&material, &plan, surface, region);
        assert_eq!(blur_stages(&chain), 10, "5 down + 5 up");
        assert_eq!(chain.len(), 11);
        assert_eq!(
            chain
                .iter()
                .filter(|stage| stage.program == ShaderProgram::KawaseDownsample)
                .count(),
            5
        );
        assert_eq!(
            chain
                .iter()
                .filter(|stage| stage.program == ShaderProgram::KawaseUpsample)
                .count(),
            5
        );
        // Back-to-front: the composite is last and covers the whole region.
        let composite = chain.last().unwrap();
        assert_eq!(composite.program, ShaderProgram::Composite);
        assert_eq!(composite.kind, PassKind::Composite);
        assert_eq!(composite.size, (DAMAGE.w, DAMAGE.h));
        let fill = composite
            .uniforms
            .iter()
            .find(|uniform| uniform.name == "fill")
            .unwrap();
        assert_eq!(fill.value, ShaderValue::Float(0.0));
    }

    #[test]
    fn down_levels_halve_and_up_levels_grow_back_to_the_region() {
        let (material, plan, surface, region) = resolved(GlassTier::Full);
        let stages = shader_stages(&material, &plan, surface, region);
        let downs: Vec<_> = stages
            .iter()
            .filter(|stage| stage.program == ShaderProgram::KawaseDownsample)
            .map(|stage| stage.size)
            .collect();
        assert_eq!(
            downs,
            vec![(512, 256), (256, 128), (128, 64), (64, 32), (32, 16)]
        );
        let ups: Vec<_> = stages
            .iter()
            .filter(|stage| stage.program == ShaderProgram::KawaseUpsample)
            .map(|stage| stage.size)
            .collect();
        assert_eq!(
            ups,
            vec![(32, 16), (64, 32), (128, 64), (256, 128), (512, 256)]
        );
    }

    #[test]
    fn reduced_tier_blurs_at_half_resolution_and_drops_refraction() {
        let (material, plan, surface, region) = resolved(GlassTier::Reduced);
        let stages = shader_stages(&material, &plan, surface, region);
        assert_eq!(blur_stages(&stages), 8, "4 down + 4 up");
        let first_down = stages
            .iter()
            .find(|stage| stage.program == ShaderProgram::KawaseDownsample)
            .unwrap();
        // Half resolution: the divisor is 2 for the very first level.
        assert_eq!(first_down.size, (DAMAGE.w / 2, DAMAGE.h / 2));

        let composite = stages.last().unwrap();
        let refraction = composite
            .uniforms
            .iter()
            .find(|uniform| uniform.name == "refraction")
            .unwrap();
        assert_eq!(refraction.value, ShaderValue::Float(0.0));

        let full = resolved(GlassTier::Full);
        let full_stages = shader_stages(&full.0, &full.1, full.2, full.3);
        let full_composite = full_stages.last().unwrap();
        let full_refraction = full_composite
            .uniforms
            .iter()
            .find(|uniform| uniform.name == "refraction")
            .unwrap();
        assert_eq!(
            full_refraction.value,
            ShaderValue::Float(REFRACTION_BAND_PX as f32)
        );
    }

    #[test]
    fn minimal_tier_is_a_single_filled_composite() {
        let (material, plan, surface, region) = resolved(GlassTier::Minimal);
        let stages = shader_stages(&material, &plan, surface, region);
        assert_eq!(stages.len(), 1, "no blur chain at Minimal");
        assert_eq!(blur_stages(&stages), 0);
        let stage = &stages[0];
        assert_eq!(stage.program, ShaderProgram::Composite);
        let uniform = |name: &str| {
            stage
                .uniforms
                .iter()
                .find(|uniform| uniform.name == name)
                .unwrap()
                .value
        };
        assert_eq!(uniform("fill"), ShaderValue::Float(1.0));
        assert_eq!(uniform("saturation"), ShaderValue::Float(1.0));
        assert_eq!(uniform("refraction"), ShaderValue::Float(0.0));
        // Solid 92 % panel colour.
        assert_eq!(uniform("tint"), ShaderValue::Vec4(0.97, 0.97, 0.98, 0.92));
    }

    #[test]
    fn composite_uniforms_carry_the_material_values() {
        let material = GlassMaterial::for_tier(GlassTier::Full);
        let uniforms = composite_uniforms(&material, surface(), DAMAGE);
        let value = |name: &str| {
            uniforms
                .iter()
                .find(|uniform| uniform.name == name)
                .unwrap()
                .value
        };
        assert_eq!(value("saturation"), ShaderValue::Float(1.2));
        assert_eq!(value("tint"), ShaderValue::Vec4(1.0, 1.0, 1.0, 0.18));
        assert_eq!(value("edge"), ShaderValue::Vec4(1.0, 0.4, 0.2, 18.0));
        assert_eq!(value("surface"), ShaderValue::Vec2(1152.0, 648.0));
        // The region is expressed inside the surface.
        assert_eq!(
            value("region"),
            ShaderValue::Vec4(64.0, 128.0, 512.0, 256.0)
        );
        assert_eq!(
            value("refraction"),
            ShaderValue::Float(REFRACTION_BAND_PX as f32)
        );
        assert_eq!(value("fill"), ShaderValue::Float(0.0));
        // Same names and order as the registered program uniforms.
        let registered: Vec<&str> = ShaderProgram::Composite
            .uniforms()
            .iter()
            .map(|(name, _)| *name)
            .collect();
        let provided: Vec<&str> = uniforms.iter().map(|uniform| uniform.name).collect();
        assert_eq!(registered, provided);
    }

    #[test]
    fn kawase_steps_are_expressed_in_source_texels() {
        let (material, plan, surface, region) = resolved(GlassTier::Full);
        let stages = shader_stages(&material, &plan, surface, region);
        // Radius 24 px over 5 levels → 4.8 texels per step, clamped to >= 0.5.
        assert_eq!(kawase_offset(24.0, 5), 4.8);
        assert_eq!(kawase_offset(0.0, 5), 0.5);
        // First step samples the full-resolution region.
        let first = &stages[0];
        assert_eq!(
            first
                .uniforms
                .iter()
                .find(|uniform| uniform.name == "sample_step")
                .unwrap()
                .value,
            ShaderValue::Vec2(4.8 / DAMAGE.w as f32, 4.8 / DAMAGE.h as f32)
        );
        // The final upsample draws into the full region but samples the level
        // below it (256x128), so its 2x taps stay inside that source level.
        let last_up = stages
            .iter()
            .rev()
            .find(|stage| stage.program == ShaderProgram::KawaseUpsample)
            .unwrap();
        assert_eq!(last_up.size, (DAMAGE.w, DAMAGE.h));
        assert_eq!(
            last_up
                .uniforms
                .iter()
                .find(|uniform| uniform.name == "sample_step")
                .unwrap()
                .value,
            ShaderValue::Vec2(4.8 / 256.0, 4.8 / 128.0)
        );
    }

    #[test]
    fn damage_region_clips_and_unions() {
        let surface = surface();
        // One strip inside the surface.
        assert_eq!(damage_region(surface, &[DAMAGE]), DAMAGE);
        // Two strips → their bounding box.
        let second = Rect {
            x: 0,
            y: 400,
            w: 100,
            h: 48,
        };
        assert_eq!(
            damage_region(surface, &[DAMAGE, second]),
            Rect {
                x: 0,
                y: 128,
                w: 576,
                h: 320
            }
        );
        // Damage outside the surface is dropped.
        assert_eq!(
            damage_region(
                surface,
                &[Rect {
                    x: 2000,
                    y: 2000,
                    w: 10,
                    h: 10
                }]
            ),
            Rect {
                x: 0,
                y: 0,
                w: 0,
                h: 0
            }
        );
        // Clipped to the surface bounds.
        assert_eq!(
            damage_region(
                surface,
                &[Rect {
                    x: 1100,
                    y: 600,
                    w: 400,
                    h: 400
                }]
            ),
            Rect {
                x: 1100,
                y: 600,
                w: 52,
                h: 48
            }
        );
    }

    #[test]
    fn shader_sources_fit_the_smithay_texture_program_contract() {
        for program in ShaderProgram::all() {
            let source = program.source();
            assert!(
                !source
                    .lines()
                    .any(|line| line.trim_start().starts_with("#version")),
                "{} must not carry a #version directive",
                program.name()
            );
            assert!(
                source.contains("//_DEFINES_"),
                "{} needs the //_DEFINES_ marker smithay replaces",
                program.name()
            );
            assert!(
                source.contains("uniform sampler2D tex;"),
                "{} samples the bound texture",
                program.name()
            );
            assert!(
                source.contains("uniform float alpha;"),
                "{} applies the renderer alpha",
                program.name()
            );
            assert!(
                source.contains("varying vec2 v_coords;"),
                "{} uses the renderer varying",
                program.name()
            );
            assert!(
                source.contains("gl_FragColor"),
                "{} is a GLSL ES 1.00 fragment shader",
                program.name()
            );
            // Every uniform the program registers is declared in the source.
            for (name, _) in program.uniforms() {
                assert!(
                    source.contains(&format!("uniform ")) && source.contains(name),
                    "{} declares {name}",
                    program.name()
                );
            }
            assert!(source.contains("precision mediump float;"));
        }
    }

    #[test]
    fn physical_material_scales_only_the_pixel_values() {
        let material = GlassMaterial::for_tier(GlassTier::Full);
        let scaled = physical_material(&material, 1.5);
        assert_eq!(scaled.corner_radius, 27);
        assert!((scaled.edge.width_px - 1.5).abs() < f32::EPSILON);
        assert_eq!(scaled.edge.alpha, material.edge.alpha);
        assert_eq!(scaled.saturation, material.saturation);
        assert_eq!(scaled.tint, material.tint);
        // Scale 1 is the identity.
        assert_eq!(physical_material(&material, 1.0), material);
    }

    #[test]
    fn stage_plan_shrinks_with_the_tier() {
        let counts: Vec<usize> = [GlassTier::Full, GlassTier::Reduced, GlassTier::Minimal]
            .into_iter()
            .map(|tier| {
                let (material, plan, surface, region) = resolved(tier);
                shader_stages(&material, &plan, surface, region).len()
            })
            .collect();
        assert_eq!(counts, vec![11, 9, 1]);
        assert!(tier_blurs(GlassTier::Full));
        assert!(tier_blurs(GlassTier::Reduced));
        assert!(!tier_blurs(GlassTier::Minimal));
    }
}
