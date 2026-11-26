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
//!    union of the glass surfaces, but only the **damage-scoped strips** of that
//!    union are rasterised each frame (see below).
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
//! **Damage scope:** the chain only rewrites the strip the frame damaged, and
//! so does the backdrop capture. The frost element covers the strip's
//! [region](crate::glass_shader::frost_region), i.e. the damage plus a halo that
//! keeps the blur away from clamped strip edges, because each frame lands in a
//! different KMS swapchain slot: a whole-surface element would repaint the
//! entire window every frame, while a strip-sized element only repaints the
//! strip the damaged content actually changed. `DrmCompositor::render_frame`
//! adds the frame's element damage to the buffer-age damage of the slot it
//! draws into, so the rest of the slot keeps the strip written into it the last
//! time that slot was used.
//!
//! The wallpaper pass is scoped the same way: [`GlassPasses::backdrop`] captures
//! exactly the regions [`crate::glass_shader::backdrop_regions`] resolves (one
//! per frosted surface, the same rects the chain samples), so a 320×100 window
//! strip costs a strip-sized clear and wallpaper draw instead of the whole union
//! of the glass surfaces. The texture keeps the union extent between frames, so
//! a capture never reallocates it and the untouched texels — which nothing
//! samples — can stay stale.
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
use smithay::reexports::wayland_server::backend::ObjectId;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::Resource;
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
    /// Region of the output (physical px) the texture covers. The texture keeps
    /// this extent between frames so steady-state frames never reallocate it;
    /// only the sub-regions in `captured` hold this frame's wallpaper.
    pub region: Rectangle<i32, Physical>,
    /// Sub-regions of `region` (texture space) rewritten this frame: the
    /// damage-scoped region of every frosted surface. Nothing outside them is
    /// sampled by the frost chain.
    pub captured: Vec<Rectangle<i32, Physical>>,
    /// Area of `captured`, i.e. the wallpaper pixels this frame rasterised.
    pub captured_px: u64,
    /// Area of `region`: what a whole-union capture would have rasterised.
    pub union_px: u64,
}

/// A frosted glass texture ready to be drawn as a render element.
#[derive(Debug, Clone)]
pub struct Frost {
    pub texture: GlesTexture,
    /// Region of the output (physical px) the texture covers: the damaged strip
    /// plus the blur halo, not the whole surface.
    pub region: Rectangle<i32, Physical>,
    /// Stable id of the surface's frost element (see [`GlassPasses::frost_id`]).
    pub id: Id,
}

impl Frost {
    /// Wrap the frost in a render element placed at its output region.
    pub fn element(&self, context: ContextId<GlesTexture>) -> DrmRenderElement {
        DrmRenderElement::Frost(TextureRenderElement::from_static_texture(
            self.id.clone(),
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
    scratch: HashMap<(i32, i32), Vec<GlesTexture>>,
    /// Targets still referenced by the previous frame's elements; they go back
    /// to the pool at the start of the next frame.
    pending: Vec<((i32, i32), GlesTexture)>,
    /// Frost element id per client surface: stable between frames so the KMS
    /// damage tracker recognises "the same element, new strip" instead of a
    /// brand-new element that has to repaint its whole geometry.
    frost_ids: HashMap<ObjectId, (Id, u64)>,
    /// Surfaces frosted since start-up.
    pub surfaces: u64,
    /// Shader passes executed since start-up.
    pub passes: u64,
    /// Offscreen textures allocated since start-up.
    pub textures: u64,
    /// Surfaces that fell back to plain composition because of a GL error.
    pub failures: u64,
    /// Physical px rewritten by the frost chain since start-up.
    pub frosted_px: u64,
    /// Physical px a whole-surface frost would have rewritten since start-up.
    pub surface_px: u64,
    /// Physical px rasterised into the backdrop since start-up.
    pub backdrop_px: u64,
    /// Physical px a whole-union backdrop capture would have rasterised.
    pub backdrop_union_px: u64,
    /// Same counters for the frame being built (`start_frame` resets them).
    pub frame_frosted_px: u64,
    pub frame_surface_px: u64,
    pub frame_backdrop_px: u64,
    pub frame_backdrop_union_px: u64,
}

impl std::fmt::Debug for GlassPasses {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GlassPasses")
            .field("surfaces", &self.surfaces)
            .field("passes", &self.passes)
            .field("textures", &self.textures)
            .field("failures", &self.failures)
            .field("frosted_px", &self.frosted_px)
            .field("surface_px", &self.surface_px)
            .field("backdrop_px", &self.backdrop_px)
            .field("backdrop_union_px", &self.backdrop_union_px)
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
            pending: Vec::new(),
            frost_ids: HashMap::new(),
            surfaces: 0,
            passes: 0,
            textures: 0,
            failures: 0,
            frosted_px: 0,
            surface_px: 0,
            backdrop_px: 0,
            backdrop_union_px: 0,
            frame_frosted_px: 0,
            frame_surface_px: 0,
            frame_backdrop_px: 0,
            frame_backdrop_union_px: 0,
        })
    }

    /// Reset the per-frame area counters and return the previous frame's
    /// targets to the pool, before the elements of a frame are built.
    pub fn start_frame(&mut self) {
        self.frame_frosted_px = 0;
        self.frame_surface_px = 0;
        self.frame_backdrop_px = 0;
        self.frame_backdrop_union_px = 0;
        for (size, texture) in std::mem::take(&mut self.pending) {
            self.scratch.entry(size).or_default().push(texture);
        }
    }

    /// Stable frost element id for one client surface.
    ///
    /// The frost of a surface moves and changes size as strips are repainted;
    /// keeping one id per `wl_surface` lets the KMS damage tracker diff the
    /// instances (and repaint exactly the old and new strip) instead of
    /// treating every frame as a freshly created element.
    pub fn frost_id(&mut self, surface: &WlSurface, frame: u64) -> Id {
        let entry = self
            .frost_ids
            .entry(surface.id())
            .or_insert_with(|| (Id::new(), frame));
        entry.1 = frame;
        entry.0.clone()
    }

    /// Forget the ids of surfaces that have not been frosted for a while (a
    /// closed window), while keeping them stable across short idle periods.
    pub fn sweep(&mut self, frame: u64) {
        const KEEP_FRAMES: u64 = 120;
        self.frost_ids
            .retain(|_, (_, last_frame)| frame.saturating_sub(*last_frame) <= KEEP_FRAMES);
    }

    /// Render the backdrop behind the given glass surfaces into one texture.
    ///
    /// `background` is the back-to-front element list of the Background/Bottom
    /// layer surfaces; `surfaces` are the logical rects that will be frosted.
    /// Only the `damage`-scoped region of each surface is rasterised (the same
    /// regions [`Self::frost`] samples from), so an idle frame does not repaint
    /// the whole union of the glass surfaces. An empty `damage` slice means a
    /// full repaint of that union. Returns `None` when nothing needs frosting or
    /// the surfaces are off screen.
    pub fn backdrop(
        &mut self,
        renderer: &mut GlesRenderer,
        background: &[WaylandSurfaceRenderElement<GlesRenderer>],
        surfaces: &[Rect],
        damage: &[Rect],
        material: &GlassMaterial,
        scale: f64,
        output_size: (i32, i32),
    ) -> Option<Backdrop> {
        let region = union_region(surfaces, scale, output_size)?;
        let halo = glass_shader::halo_px(&glass_shader::physical_material(material, scale));
        let scoped = glass_shader::backdrop_regions(surfaces, damage, scale, halo, output_size);
        // Texture space: capture sub-regions are output coordinates shifted to
        // the texture origin, and can never exceed the union the texture covers.
        let captured: Vec<Rectangle<i32, Physical>> = scoped
            .iter()
            .filter_map(|rect| {
                Rectangle::<i32, Physical>::new(
                    Point::from((rect.x - region.loc.x, rect.y - region.loc.y)),
                    Size::from((rect.w, rect.h)),
                )
                .intersection(Rectangle::from_size(region.size))
            })
            .collect();
        let captured_px: u64 = captured
            .iter()
            .map(|rect| rect.size.w.max(0) as u64 * rect.size.h.max(0) as u64)
            .sum();
        let union_px = region.size.w.max(0) as u64 * region.size.h.max(0) as u64;
        match self.capture(renderer, background, region, &captured) {
            Ok(texture) => {
                self.backdrop_px += captured_px;
                self.backdrop_union_px += union_px;
                self.frame_backdrop_px += captured_px;
                self.frame_backdrop_union_px += union_px;
                Some(Backdrop {
                    texture,
                    region,
                    captured,
                    captured_px,
                    union_px,
                })
            }
            Err(err) => {
                self.failures += 1;
                eprintln!("aether-compositor: glass backdrop: {err:?}");
                None
            }
        }
    }

    /// Run the blur and composite chain for one surface.
    ///
    /// `damage` is the frame damage in logical coordinates (empty = full
    /// repaint). The chain only runs over the surface's damage-scoped region
    /// ([`crate::glass_shader::frost_region_scaled`]), so an idle strip costs a
    /// strip-sized blur. Returns `None` when the surface has no damage this
    /// frame, GL failed, or the surface has no visible area.
    #[allow(clippy::too_many_arguments)]
    pub fn frost(
        &mut self,
        renderer: &mut GlesRenderer,
        backdrop: Option<&Backdrop>,
        material: &GlassMaterial,
        plan: &EffectPlan,
        surface: Rect,
        damage: &[Rect],
        scale: f64,
        id: Id,
    ) -> Option<Frost> {
        if surface.w <= 0 || surface.h <= 0 {
            return None;
        }
        // A blur tier without a captured backdrop (capture failed this frame)
        // falls back to plain composition: the fill path would sample the
        // placeholder and paint a wrong colour.
        if backdrop.is_none() && material.blur.is_some() {
            return None;
        }

        let output_rect = physical(surface, scale);
        if output_rect.size.w <= 0 || output_rect.size.h <= 0 {
            return None;
        }
        let physical_material = glass_shader::physical_material(material, scale);

        // The strip this frame has to rewrite, in surface-local physical px.
        let local = glass_shader::frost_region_scaled(
            surface,
            damage,
            scale,
            glass_shader::halo_px(&physical_material),
        )?;

        // Output-space region. It is clipped to the captured backdrop because
        // that texture is what the blur samples.
        let mut region = Rectangle::<i32, Physical>::new(
            Point::from((output_rect.loc.x + local.x, output_rect.loc.y + local.y)),
            Size::from((local.w, local.h)),
        );
        if let Some(backdrop) = backdrop {
            region = region.intersection(backdrop.region)?;
            // The backdrop only holds the strips captured this frame; the same
            // `backdrop_regions` call produced both, so the strip must be one of
            // them (a mismatch would blur stale or unwritten texels).
            debug_assert!(
                backdrop
                    .captured
                    .iter()
                    .any(|strip| strip.intersection(region) == Some(region)),
                "frost region {region:?} is not inside a captured backdrop strip"
            );
        }
        // Stage sizes, uniforms and the composite's region are surface-local.
        let shader_region = Rect {
            x: region.loc.x - output_rect.loc.x,
            y: region.loc.y - output_rect.loc.y,
            w: region.size.w,
            h: region.size.h,
        };

        // The source patch of the backdrop under this strip. The Minimal tier
        // samples nothing (`fill = 1`), so a 1x1 placeholder is enough.
        let (mut source, mut source_rect) = match backdrop {
            Some(backdrop) => (
                backdrop.texture.clone(),
                Rectangle::<f64, Buffer>::new(
                    Point::from((
                        (region.loc.x - backdrop.region.loc.x) as f64,
                        (region.loc.y - backdrop.region.loc.y) as f64,
                    )),
                    Size::<f64, Buffer>::from((region.size.w as f64, region.size.h as f64)),
                ),
            ),
            None => (
                self.empty.clone(),
                Rectangle::from_size(Size::<i32, Buffer>::from((1, 1)).to_f64()),
            ),
        };

        // Shader sizes and the material's pixel values are physical px; the
        // surface rect stays whole because the composite pass needs it for the
        // rim and the corner mask.
        let logical_surface = Rect {
            x: 0,
            y: 0,
            w: output_rect.size.w,
            h: output_rect.size.h,
        };
        let logical_region = shader_region;
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
                self.scratch.entry(size).or_default().push(previous);
            }
            // The next stage samples this whole target.
            source_rect = Rectangle::from_size(target.size().to_f64());
            source = target.clone();
            owned = Some((stage.size, target));
        }
        // The last target is referenced by the element this frame pushes, so it
        // only goes back to the pool once that frame is done.
        if let Some((size, texture)) = owned.take() {
            self.pending.push((size, texture));
        }

        self.surfaces += 1;
        self.passes += stages.len() as u64;
        let frosted = (region.size.w * region.size.h).max(0) as u64;
        let full = (output_rect.size.w * output_rect.size.h).max(0) as u64;
        self.frosted_px += frosted;
        self.surface_px += full;
        self.frame_frosted_px += frosted;
        self.frame_surface_px += full;

        Some(Frost {
            texture: source,
            region,
            id,
        })
    }

    /// One-line marker with the counters, for `AETHER_COMPOSITOR_GLASS_SHADER`.
    pub fn stats_line(&self) -> String {
        format!(
            "surfaces={} passes={} textures={} failures={} frosted_px={} surface_px={} backdrop_px={} backdrop_union_px={}",
            self.surfaces,
            self.passes,
            self.textures,
            self.failures,
            self.frosted_px,
            self.surface_px,
            self.backdrop_px,
            self.backdrop_union_px
        )
    }

    fn program(&self, program: ShaderProgram) -> &GlesTexProgram {
        match program {
            ShaderProgram::KawaseDownsample => &self.down,
            ShaderProgram::KawaseUpsample => &self.up,
            ShaderProgram::Composite => &self.composite,
        }
    }

    /// Rasterise the backdrop for `captured` (texture space) into `region`.
    ///
    /// Only the given sub-regions are cleared and drawn, so the wallpaper pass
    /// costs the damage, not the union of the glass surfaces. The texture itself
    /// keeps the union extent between frames: leaving the other texels alone is
    /// safe because the chain only samples inside `captured`, and it avoids
    /// reallocating the backdrop whenever a window moves. The very first frame
    /// (and every frame after a VT switch or `reset_buffers`) damages the whole
    /// output, so a new texture is always written in full before anything can
    /// sample it.
    fn capture(
        &mut self,
        renderer: &mut GlesRenderer,
        background: &[WaylandSurfaceRenderElement<GlesRenderer>],
        region: Rectangle<i32, Physical>,
        captured: &[Rectangle<i32, Physical>],
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
        if !captured.is_empty() {
            let damage = captured;
            let mut target = renderer.bind(&mut texture)?;
            let mut frame = renderer.render(&mut target, target_size, Transform::Normal)?;
            // The clear colour matches the compositor's own clear so an area
            // without wallpaper blurs to the same colour it composites on.
            frame.clear([0.0f32, 0.0, 0.0, 1.0].into(), damage)?;
            for element in background {
                let geometry = element.geometry(Scale::from(1.0f64));
                if geometry.size.w <= 0 || geometry.size.h <= 0 {
                    continue;
                }
                // Only the captured strips of elements that reach them.
                let dst = Rectangle::new(geometry.loc - region.loc, geometry.size);
                let visible: Vec<Rectangle<i32, Physical>> = damage
                    .iter()
                    .filter_map(|strip| strip.intersection(dst))
                    .collect();
                if visible.is_empty() {
                    continue;
                }
                element.draw(&mut frame, element.src(), dst, &visible, &[])?;
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
        if let Some(textures) = self.scratch.get_mut(&size) {
            if let Some(texture) = textures.pop() {
                return Ok(texture);
            }
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
