//! Screenshot regression tests for Fytti's HTML/CSS renderer.
//!
//! Renders each fixture HTML at 800x600, compares pixel-by-pixel against
//! reference PNGs. Fails if more than 0.1% of pixels differ.
//!
//! To update references after intentional changes:
//!   cargo run -- tests/fixtures/NAME.html --png tests/references/NAME.png --width 800 --height 600

use std::path::Path;

fn render_to_pixels(html_path: &str, width: u32, height: u32) -> Vec<u8> {
    let html = std::fs::read_to_string(html_path)
        .unwrap_or_else(|e| panic!("Failed to read {html_path}: {e}"));

    let doc = fytti_html::parse(&html);
    let styles = fytti_css::resolve(&doc);

    let mut renderer = fytti_render::Renderer::new(width, height);

    let body = doc.body();
    let body_style = styles.get(&body).cloned().unwrap_or_default();
    renderer.clear(body_style.background_color);

    let layout =
        fytti_layout::layout(&doc, &styles, width as f32, height as f32, &mut renderer);
    renderer.paint(&layout, &doc, &styles);

    renderer.pixels().to_vec()
}

fn load_reference_png(path: &str) -> (Vec<u8>, u32, u32) {
    let data = std::fs::read(path).unwrap_or_else(|e| panic!("Failed to read {path}: {e}"));
    let decoder = png::Decoder::new(std::io::Cursor::new(data));
    let mut reader = decoder.read_info().expect("PNG decode failed");
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).expect("PNG frame failed");
    buf.truncate(info.buffer_size());
    (buf, info.width, info.height)
}

fn compare_pixels(actual: &[u8], reference: &[u8], width: u32, height: u32) -> f64 {
    assert_eq!(
        actual.len(),
        reference.len(),
        "pixel buffer size mismatch: {} vs {}",
        actual.len(),
        reference.len()
    );

    let total = (width * height) as usize;
    let mut diff_count = 0usize;

    for i in 0..total {
        let base = i * 4;
        if base + 3 >= actual.len() {
            break;
        }
        // Allow small per-channel tolerance (font rendering varies)
        let dr = (actual[base] as i32 - reference[base] as i32).abs();
        let dg = (actual[base + 1] as i32 - reference[base + 1] as i32).abs();
        let db = (actual[base + 2] as i32 - reference[base + 2] as i32).abs();
        if dr > 2 || dg > 2 || db > 2 {
            diff_count += 1;
        }
    }

    diff_count as f64 / total as f64 * 100.0
}

fn run_screenshot_test(fixture: &str) {
    run_screenshot_test_full(fixture, fixture, 800, 600);
}

fn run_screenshot_test_at_size(html_fixture: &str, ref_name: &str, width: u32, height: u32) {
    run_screenshot_test_full(html_fixture, ref_name, width, height);
}

fn run_screenshot_test_full(html_fixture: &str, ref_name: &str, width: u32, height: u32) {
    let html_path = format!("tests/fixtures/{html_fixture}.html");
    let ref_path = format!("tests/references/{ref_name}.png");

    let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let html_path = workspace_root.join(&html_path).to_string_lossy().to_string();
    let ref_path = workspace_root.join(&ref_path).to_string_lossy().to_string();

    assert!(
        Path::new(&ref_path).exists(),
        "Reference PNG missing: {ref_path}. Generate with: cargo run -- {html_path} --png {ref_path} --width {width} --height {height}"
    );

    let actual = render_to_pixels(&html_path, width, height);
    let (reference, ref_w, ref_h) = load_reference_png(&ref_path);

    assert_eq!(ref_w, width, "reference width mismatch");
    assert_eq!(ref_h, height, "reference height mismatch");

    let diff_pct = compare_pixels(&actual, &reference, width, height);
    assert!(
        diff_pct < 0.1,
        "{ref_name}: {diff_pct:.2}% pixels differ (threshold: 0.1%)"
    );
}

#[test]
fn screenshot_basic_text() {
    run_screenshot_test("basic-text");
}

#[test]
fn screenshot_colors_and_boxes() {
    run_screenshot_test("colors-and-boxes");
}

#[test]
fn screenshot_nested_layout() {
    run_screenshot_test("nested-layout");
}

#[test]
fn screenshot_heading_sizes() {
    run_screenshot_test("heading-sizes");
}

#[test]
fn screenshot_resize_stress() {
    run_screenshot_test("resize-stress");
}

#[test]
fn screenshot_resize_small() {
    run_screenshot_test_at_size("resize-stress", "resize-stress-small", 400, 300);
}

#[test]
fn screenshot_resize_wide() {
    run_screenshot_test_at_size("resize-stress", "resize-stress-wide", 1200, 400);
}
