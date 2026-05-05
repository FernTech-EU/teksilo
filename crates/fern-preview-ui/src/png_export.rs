//! Off-screen PNG snapshot of the current canvas.
//!
//! Reuses the production `fern-render::Renderer` against a detached
//! `wgpu::TextureView`. The pipeline mirrors fern-render's headless
//! test infrastructure (`fern_render::test_support`):
//!
//! 1. Construct a fresh `WidgetTree` with the canvas's theme, add the
//!    catalog entry's `build(variant, knobs)` widget as its root.
//! 2. Run layout at a fixed proposal size (default 1024 × 768).
//! 3. Spin up an off-screen `wgpu::Texture`, generate a `RenderFrame`
//!    via `WidgetTree::render`, render to the texture.
//! 4. Read back via `copy_texture_to_buffer` and encode PNG.
//!
//! The result is the path the PNG was saved to.

use std::path::PathBuf;

use fern_canvas::SizeProposal;
use fern_core::widget_tree::WidgetTree;
use fern_render::test_support;

use crate::app_state::AppState;

const DEFAULT_TEXTURE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const DEFAULT_CANVAS_SIZE: (u32, u32) = (1024, 768);

/// Export the currently selected (widget, variant) to a PNG at
/// `~/.fern-previewer/exports/<widget>__<variant>__<theme>.png`. The
/// directory is created on demand. Returns the saved path or a
/// human-readable error.
pub fn export_current(state: &AppState) -> Result<PathBuf, String> {
    let (widget_id, variant_name) =
        match (state.selected_widget.get(), state.selected_variant.get()) {
            (Some(w), Some(v)) => (w, v),
            _ => return Err("no widget/variant selected".into()),
        };

    let entry = fern_preview::find_by_id(widget_id)
        .ok_or_else(|| format!("no entry registered with id '{}'", widget_id))?;
    let knobs = state.knobs_for(widget_id, variant_name);
    let widget = entry.build(variant_name, &knobs);

    let canvas_theme = state.canvas_theme.get();
    let theme = canvas_theme.theme();
    let (width, height) = DEFAULT_CANVAS_SIZE;

    let mut tree = WidgetTree::new().with_theme(theme);
    let _root = tree.add_boxed(widget);
    tree.layout(SizeProposal::exact(width as f32, height as f32));
    let frame = tree.render();

    let bytes = pollster::block_on(render_offscreen(&frame, width, height))?;
    let out_path = output_path(widget_id, variant_name, canvas_theme)?;
    encode_rgba_to_png(&out_path, &bytes, width, height)?;
    Ok(out_path)
}

fn output_path(
    widget_id: &'static str,
    variant_name: &'static str,
    canvas_theme: crate::app_state::CanvasTheme,
) -> Result<PathBuf, String> {
    let mut out_dir = home_dir().ok_or_else(|| "couldn't resolve home directory".to_string())?;
    out_dir.push(".fern-previewer");
    out_dir.push("exports");
    std::fs::create_dir_all(&out_dir).map_err(|e| format!("create_dir_all: {}", e))?;
    // Resolve the chosen theme to its concrete light/dark identity
    // for the filename. `Native` resolves at click-time via
    // `theme()`; we mirror that resolution here so an export taken
    // while "Native" is selected still gets a meaningful suffix
    // ("native-light" / "native-dark") rather than a bare "native"
    // that drops information about what was actually rendered.
    let theme_label: &'static str = match canvas_theme {
        crate::app_state::CanvasTheme::Light => "light",
        crate::app_state::CanvasTheme::Dark => "dark",
        crate::app_state::CanvasTheme::Native => {
            if fern_platform::os_theme::query_color_scheme().is_dark() {
                "native-dark"
            } else {
                "native-light"
            }
        }
    };
    Ok(out_dir.join(format!(
        "{}__{}__{}.png",
        widget_id, variant_name, theme_label
    )))
}

fn home_dir() -> Option<PathBuf> {
    if let Ok(home) = std::env::var("HOME")
        && !home.is_empty()
    {
        return Some(PathBuf::from(home));
    }
    #[cfg(windows)]
    if let Ok(home) = std::env::var("USERPROFILE") {
        if !home.is_empty() {
            return Some(PathBuf::from(home));
        }
    }
    None
}

async fn render_offscreen(
    frame: &fern_canvas::RenderFrame,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, String> {
    let (mut renderer, device, queue) =
        match test_support::create_test_renderer("fern-preview-ui png export").await {
            Some(t) => t,
            None => {
                return Err(
                    "wgpu adapter unavailable — no GPU backend present for snapshot rendering"
                        .into(),
                );
            }
        };

    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("fern-preview-ui export texture"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEFAULT_TEXTURE_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

    renderer.render(frame, &view, 1.0, width, height, [0.0, 0.0, 0.0, 0.0]);

    Ok(test_support::read_texture_rgba(
        &device, &queue, &texture, width, height,
    ))
}

fn encode_rgba_to_png(
    path: &std::path::Path,
    rgba: &[u8],
    width: u32,
    height: u32,
) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| format!("create {:?}: {}", path, e))?;
    let writer = std::io::BufWriter::new(file);
    let mut encoder = png::Encoder::new(writer, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|e| format!("png header: {}", e))?;
    writer
        .write_image_data(rgba)
        .map_err(|e| format!("png write: {}", e))?;
    Ok(())
}
