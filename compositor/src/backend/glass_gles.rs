//! P3-T05: GLES execution of the glass shader passes.
//!
//! [`crate::glass_shader`] resolves a tier into a program sequence and the
//! uniform values for one surface; this module runs that sequence on the
//! `drm` feature path with the smithay `GlesRenderer` that also drives the
//! KMS primary plane ([`super::drm`]).
//!
//! Per frame:
//!
//! 1. **Backdrop capture** — the background/bottom layer surfaces (wallpaper)
//!    plus the clear colour are rendered into an offscreen texture covering the
//!    union of the glass surfaces.
//! 2. **Dual-Kawase blur** — for every glass surface the backdrop patch under
//!    it is downsampled through N levels and reconstructed through N upsample
//!    passes at the sizes [`crate::glass_shader::shader_stages`] computed
//!    (Full: 5+5 at full resolution, Reduced: 4+4 at half resolution).
//! 3. **Composite** — one fused pass applies saturation, tint, edge refraction,
//!    the 1 px rim highlight, the rounded-corner mask and the premultiplied
//!    alpha, or (Minimal tier) draws the solid tinted panel with `fill = 1`.
//! 4. The resulting texture is drawn under the surface's own content as a
//!    [`DrmRenderElement::Frost`] element.
//!
//! Only the `drm` path compiles this module, so the headless backend, the
//! unit tests and CI stay GPU-free; the shader sources are still checked by
//! `glass_shader`'s tests, and `compositor/scripts/check-linux-target.sh`
//! type-checks the GL code for the device target.
//!
//! **Damage scope:** [`super::drm`] renders every element into a fresh
//! swapchain slot each frame (`DrmCompositor::render_frame` without an
//! `OutputDamageTracker`), so the frost element must cover the whole surface —
//! a strip-sized frost would leave the rest of the surface empty in the new
//! slot. The damage region still decides *whether* a surface is frosted this
//! frame and is reported in the markers; scoping the blur to the damaged strip
//! is the remaining piece of P3-T06 (needs buffer-age tracking).
//!
//! Failure policy: any GL error in the chain is logged, counted in
//! [`GlassPasses::failures`] and the surface falls back to plain composition
//! for that frame — a broken shader must not take the desktop down.

use std::collections::HashMap;

use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::element::surface::WaylandSurfaceRenderElement;
use smithay::backend::renderer::element::texture::TextureRenderElement;
use smithay::backend::renderer::element::{Element, Id, Kind, RenderElement};
use smithay::backend::renderer::gles::{
    GlesError, GlesRenderer, GlesTexProgram, GlesTexture, Uniform, UniformName, UniformType,
};
use smithay::backend::renderer::{Bind, ContextId, Frame, Offscreen, Renderer, Texture};
use smithay::utils::{Buffer, Physical, Point, Rectangle, Scale, Size, Transform};

use crate::glass::{EffectPlan, GlassMaterial};
use crate::glass_shader::{self, ShaderProgram, ShaderStage, ShaderValue};
use crate::wm::Rect;

smithay::backend::renderer::element::render_elements! {
    /// Elements the DRM composite path submits to KMS.
    pub DrmRenderElement<=GlesRenderer>;
    /// A client buffer (toplevel, layer surface or cursor).
    Surface=WaylandSurfaceRenderElement<GlesRenderer>,
    /// The frosted glass texture drawn underneath a glass surface.
    Frost=TextureRenderElement<GlesTexture>,
}

/// The captured backdrop: an offscreen texture plus the output region it covers.
#[derive(Debug, Clone)]
pub struct Backdrop {
    pub texture: GlesTexture,
    /// Region of the output (physical px) the texture covers.
    pub region: Rectangle<i32, Physical>,
}

/// A frosted glass texture ready to be drawn as a render element.
#[derive(Debug, Clone)]
pub struct Frost {
    pub texture: GlesTexture,
    /// Region of the output (physical px) the texture covers.
    pub region: Rectangle<i32, Physical>,
}

impl Frost {
    /// Wrap the frost in a render element placed at its output region.
    pub fn element(&self, context: ContextId<GlesTexture>) -> DrmRenderElement {
        DrmRenderElement::Frost(TextureRenderElement::from_static_texture(
            Id::new(),
            context,
            Point::<f64, Physical>::from((self.region.loc.x as f64, self.region.loc.y as f64)),
            self.texture.clone(),
            1,
            Transform::Normal,
            Some(1.0),
            None,
            None,
            None,
            // Unspecified: the frost may sit under a cursor, so it is not
            // scanned out separately.
            Kind::Unspecified,
        ))
    }
}

/// Compiled glass programs plus the counters reported in the DRM markers.
pub struct GlassPasses {
    down: GlesTexProgram,
    up: GlesTexProgram,
    composite: GlesTexProgram,
    /// 1x1 placeholder bound when the Minimal tier samples no backdrop.
    empty: GlesTexture,
    /// Backdrop texture reused across frames (reallocated only on size change).
    backdrop: Option<GlesTexture>,
    /// Offscreen targets pooled by pixel size, so steady-state frames allocate
    /// no textures at all.
    scratch: HashMap<(i32, i32), GlesTexture>,
    /// Surfaces frosted since start-up.
    pub surfaces: u64,
    /// Shader passes executed since start-up.
    pub passes: u64,
    /// Offscreen textures allocated since start-up.
    pub textures: u64,
    /// Surfaces that fell back to plain composition because of a GL error.
    pub failures: u64,
}

impl std::fmt::Debug for GlassPasses {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GlassPasses")
            .field("surfaces", &self.surfaces)
            .field("passes", &self.passes)
            .field("textures", &self.textures)
            .field("failures", &self.failures)
            .finish()
    }
}

impl GlassPasses {
    /// Compile the three glass programs on the renderer's EGL context.
    pub fn new(renderer: &mut GlesRenderer) -> Result<Self, GlesError> {
        Ok(Self {
            down: compile(renderer, ShaderProgram::KawaseDownsample)?,
            up: compile(renderer, ShaderProgram::KawaseUpsample)?,
            composite: compile(renderer, ShaderProgram::Composite)?,
            empty: renderer.create_buffer(Fourcc::Argb8888, Size::from((1, 1)))?,
            backdrop: None,
            scratch: HashMap::new(),
            surfaces: 0,
            passes: 0,
            textures: 0,
            failures: 0,
        })
    }

    /// Render the backdrop behind the given glass surfaces into one texture.
    ///
    /// `background` is the back-to-front element list of the Background/Bottom
    /// layer surfaces; `surfaces` are the logical rects that will be frosted.
    /// Returns `None` when nothing needs frosting or the surfaces are off
    /// screen.
    pub fn backdrop(
        &mut self,
        renderer: &mut GlesRenderer,
        background: &[WaylandSurfaceRenderElement<GlesRenderer>],
        surfaces: &[Rect],
        scale: f64,
        output_size: (i32, i32),
    ) -> Option<Backdrop> {
        let region = union_region(surfaces, scale, output_size)?;
        match self.capture(renderer, background, region) {
            Ok(texture) => Some(Backdrop { texture, region }),
            Err(err) => {
                self.failures += 1;
                eprintln!("aether-compositor: glass backdrop: {err:?}");
                None
            }
        }
    }

    /// Run the blur and composite chain for one surface.
    ///
    /// `damage` is the frame damage in logical coordinates (empty = whole
    /// surface). Returns `None` when the surface has no damage this frame, GL
    /// failed, or the surface has no visible area.
    pub fn frost(
        &mut self,
        renderer: &mut GlesRenderer,
        backdrop: Option<&Backdrop>,
        material: &GlassMaterial,
        plan: &EffectPlan,
        surface: Rect,
        damage: &[Rect],
        scale: f64,
    ) -> Option<Frost> {
        if surface.w <= 0 || surface.h <= 0 {
            return None;
        }
        // Damage only decides whether this surface is frosted this frame; see
        // the module docs for why the frost itself covers the whole surface.
        if !damage.is_empty() {
            let damaged = glass_shader::damage_region(surface, damage);
            if damaged.w <= 0 || damaged.h <= 0 {
                return None;
            }
        }
        // A blur tier without a captured backdrop (capture failed this frame)
        // falls back to plain composition: the fill path would sample the
        // placeholder and paint a wrong colour.
        if backdrop.is_none() && material.blur.is_some() {
            return None;
        }

        let output_rect = physical(surface, scale);
        let region = output_rect;
        // The source patch of the backdrop under this surface. The Minimal
        // tier samples nothing (`fill = 1`), so a 1x1 placeholder is enough.
        let (mut source, mut source_rect) = match backdrop {
            Some(backdrop) => (
                backdrop.texture.clone(),
                Rectangle::<f64, Buffer>::new(
                    Point::from((
                        (output_rect.loc.x - backdrop.region.loc.x) as f64,
                        (output_rect.loc.y - backdrop.region.loc.y) as f64,
                    )),
                    Size::<f64, Buffer>::from((
                        output_rect.size.w as f64,
                        output_rect.size.h as f64,
                    )),
                ),
            ),
            None => (
                self.empty.clone(),
                Rectangle::from_size(Size::<i32, Buffer>::from((1, 1)).to_f64()),
            ),
        };

        // Shader sizes and the material's pixel values are physical px.
        let physical_material = glass_shader::physical_material(material, scale);
        let logical_surface = Rect {
            x: 0,
            y: 0,
            w: output_rect.size.w,
            h: output_rect.size.h,
        };
        let logical_region = logical_surface;
        let stages =
            glass_shader::shader_stages(&physical_material, plan, logical_surface, logical_region);

        // The chain reuses pooled offscreen targets, so a steady-state frame
        // allocates nothing. `owned` tracks the scratch texture currently being
        // sampled so it can go back to the pool once the next pass consumed it.
        let mut owned: Option<((i32, i32), GlesTexture)> = None;
        for stage in &stages {
            let mut target = match self.target(renderer, stage.size) {
                Ok(texture) => texture,
                Err(err) => {
                    self.failures += 1;
                    eprintln!("aether-compositor: glass target: {err:?}");
                    return None;
                }
            };
            let program = self.program(stage.program);
            if let Err(err) = draw_pass(
                renderer,
                program,
                &source,
                source_rect,
                &mut target,
                &stage_uniforms(stage),
            ) {
                self.failures += 1;
                eprintln!("aether-compositor: glass {}: {err:?}", stage.program.name());
                return None;
            }
            // The pass sampled the previous stage: it can go back to the pool.
            if let Some((size, previous)) = owned.take() {
                self.scratch.insert(size, previous);
            }
            // The next stage samples this whole target.
            source_rect = Rectangle::from_size(target.size().to_f64());
            source = target.clone();
            owned = Some((stage.size, target));
        }
        // `owned` still holds the last target: it stays the frost texture and
        // is not returned to the pool while the frame references it.

        self.surfaces += 1;
        self.passes += stages.len() as u64;

        Some(Frost {
            texture: source,
            region,
        })
    }

    /// One-line marker with the counters, for `AETHER_COMPOSITOR_GLASS_SHADER`.
    pub fn stats_line(&self) -> String {
        format!(
            "surfaces={} passes={} textures={} failures={}",
            self.surfaces, self.passes, self.textures, self.failures
        )
    }

    fn program(&self, program: ShaderProgram) -> &GlesTexProgram {
        match program {
            ShaderProgram::KawaseDownsample => &self.down,
            ShaderProgram::KawaseUpsample => &self.up,
            ShaderProgram::Composite => &self.composite,
        }
    }

    fn capture(
        &mut self,
        renderer: &mut GlesRenderer,
        background: &[WaylandSurfaceRenderElement<GlesRenderer>],
        region: Rectangle<i32, Physical>,
    ) -> Result<GlesTexture, GlesError> {
        let size = region.size;
        let buffer_size = Size::<i32, Buffer>::from((size.w, size.h));
        let target_size = Size::<i32, Physical>::from((size.w, size.h));
        let mut texture = match self.backdrop.take() {
            Some(texture) if texture.size() == buffer_size => texture,
            _ => {
                self.textures += 1;
                renderer.create_buffer(Fourcc::Argb8888, buffer_size)?
            }
        };
        {
            let mut target = renderer.bind(&mut texture)?;
            let mut frame = renderer.render(&mut target, target_size, Transform::Normal)?;
            let damage = [Rectangle::from_size(target_size)];
            // The clear colour matches the compositor's own clear so an area
            // without wallpaper blurs to the same colour it composites on.
            frame.clear([0.0f32, 0.0, 0.0, 1.0].into(), &damage)?;
            for element in background {
                let geometry = element.geometry(Scale::from(1.0f64));
                if geometry.size.w <= 0 || geometry.size.h <= 0 {
                    continue;
                }
                // Only elements that overlap the captured region contribute.
                if geometry.intersection(region).is_none() {
                    continue;
                }
                let dst = Rectangle::new(geometry.loc - region.loc, geometry.size);
                element.draw(&mut frame, element.src(), dst, &damage, &[])?;
            }
            let _ = frame.finish()?;
        }
        // Keep the texture for the next frame; the clone shares the GL handle.
        self.backdrop = Some(texture.clone());
        Ok(texture)
    }

    /// Take a pooled target of `size`, creating one only when the pool is dry.
    fn target(
        &mut self,
        renderer: &mut GlesRenderer,
        size: (i32, i32),
    ) -> Result<GlesTexture, GlesError> {
        if let Some(texture) = self.scratch.remove(&size) {
            return Ok(texture);
        }
        self.textures += 1;
        renderer.create_buffer(Fourcc::Argb8888, Size::<i32, Buffer>::from(size))
    }
}

fn compile(
    renderer: &mut GlesRenderer,
    program: ShaderProgram,
) -> Result<GlesTexProgram, GlesError> {
    let uniforms: Vec<UniformName<'static>> = program
        .uniforms()
        .iter()
        .map(|(name, kind)| {
            let kind = match kind {
                glass_shader::UniformKind::Float => UniformType::_1f,
                glass_shader::UniformKind::Vec2 => UniformType::_2f,
                glass_shader::UniformKind::Vec4 => UniformType::_4f,
            };
            UniformName::new(*name, kind)
        })
        .collect();
    renderer.compile_custom_texture_shader(program.source(), &uniforms)
}

/// Translate one stage's neutral uniforms into smithay's GL uniforms.
fn stage_uniforms(stage: &ShaderStage) -> Vec<Uniform<'static>> {
    stage
        .uniforms
        .iter()
        .map(|uniform| match uniform.value {
            ShaderValue::Float(value) => Uniform::new(uniform.name, value),
            ShaderValue::Vec2(x, y) => Uniform::new(uniform.name, [x, y]),
            ShaderValue::Vec4(x, y, z, w) => Uniform::new(uniform.name, [x, y, z, w]),
        })
        .collect()
}

/// Draw one textured pass into an offscreen target.
fn draw_pass(
    renderer: &mut GlesRenderer,
    program: &GlesTexProgram,
    source: &GlesTexture,
    source_rect: Rectangle<f64, Buffer>,
    target: &mut GlesTexture,
    uniforms: &[Uniform<'static>],
) -> Result<(), GlesError> {
    let size = target.size();
    if size.w <= 0 || size.h <= 0 {
        return Ok(());
    }
    let target_size = Size::<i32, Physical>::from((size.w, size.h));
    let mut bound = renderer.bind(target)?;
    {
        let mut frame = renderer.render(&mut bound, target_size, Transform::Normal)?;
        let damage = [Rectangle::from_size(target_size)];
        frame.clear([0.0f32, 0.0, 0.0, 0.0].into(), &damage)?;
        frame.render_texture_from_to(
            source,
            source_rect,
            Rectangle::from_size(target_size),
            &damage,
            &[],
            Transform::Normal,
            1.0,
            Some(program),
            uniforms,
        )?;
        let _ = frame.finish()?;
    }
    Ok(())
}

/// Union of the logical rects in physical output pixels, clipped to the output.
fn union_region(
    rects: &[Rect],
    scale: f64,
    output_size: (i32, i32),
) -> Option<Rectangle<i32, Physical>> {
    let mut union: Option<Rectangle<i32, Physical>> = None;
    let output = Rectangle::from_size(Size::<i32, Physical>::from(output_size));
    for rect in rects {
        let Some(physical) = physical(*rect, scale).intersection(output) else {
            continue;
        };
        if physical.size.w <= 0 || physical.size.h <= 0 {
            continue;
        }
        union = Some(match union {
            None => physical,
            Some(current) => {
                let x0 = current.loc.x.min(physical.loc.x);
                let y0 = current.loc.y.min(physical.loc.y);
                let x1 = (current.loc.x + current.size.w).max(physical.loc.x + physical.size.w);
                let y1 = (current.loc.y + current.size.h).max(physical.loc.y + physical.size.h);
                Rectangle::new(Point::from((x0, y0)), Size::from((x1 - x0, y1 - y0)))
            }
        });
    }
    union
}

/// Logical rect -> physical output rect.
fn physical(rect: Rect, scale: f64) -> Rectangle<i32, Physical> {
    Rectangle::new(
        Point::from((
            (rect.x as f64 * scale).round() as i32,
            (rect.y as f64 * scale).round() as i32,
        )),
        Size::from((
            (rect.w as f64 * scale).round().max(0.0) as i32,
            (rect.h as f64 * scale).round().max(0.0) as i32,
        )),
    )
}
