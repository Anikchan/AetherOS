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
//! section 4.

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
