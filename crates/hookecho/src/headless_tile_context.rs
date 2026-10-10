//! GPU checks for timed raster cache and pyramid fallback isolation.
use super::*;
use crate::render::{PendingTile, RasterContext, TileId, VisibleTile};

const SIZE: u32 = 200;
fn style() -> u8 {
    BasemapStyle::GoesEastIR.key()
}
const ID: TileId = (6, 14, 24);

fn callback(pane: u32, context: RasterContext) -> MapCallback {
    let camera = Camera {
        center: (14.5 / 64.0, 24.5 / 64.0),
        ..Camera::at_lonlat(-97.0, 35.0, 6.0)
    };
    let viewport = (SIZE as f32, SIZE as f32);
    let (center, scale) = camera.world_to_clip_uniform(viewport);
    MapCallback {
        pane,
        camera_center: center,
        camera_scale: scale,
        world_per_pixel: camera.world_per_pixel() as f32,
        camera_view_proj: camera.view_projection_uniform(viewport),
        camera_3d: 0.0,
        camera_globe: [0.0; 4],
        basemap_key: style(),
        basemap_context: context,
        vector_over_raster: false,
        new_tiles: Vec::new(),
        visible: vec![VisibleTile {
            id: ID,
            world_min: [14.0 / 64.0, 24.0 / 64.0],
            world_max: [15.0 / 64.0, 25.0 / 64.0],
        }],
        radar_upload: None,
        draw_radar: false,
        observed_upload: None,
        draw_observed: false,
        overlay_upload: None,
        draw_overlay: false,
        field_uploads: Vec::new(),
        model_uploads: Vec::new(),
        model_fields: Vec::new(),
        drop_model_fields: Vec::new(),
        mrms_uploads: Vec::new(),
        mrms_fields: Vec::new(),
        drop_mrms_fields: Vec::new(),
        field_draws: Vec::new(),
        field_swipe: None,
        field_fades: Vec::new(),
        clear_tiles: false,
        drop_tiles: Vec::new(),
        drop_fields: Vec::new(),
        new_vector_tiles: Vec::new(),
        visible_vector: Vec::new(),
        clear_vector: false,
        drop_vector_tiles: Vec::new(),
        wind_upload: None,
        wind: None,
    }
}

fn upload(context: RasterContext, id: TileId, color: [u8; 4]) -> PendingTile {
    PendingTile {
        style: style(),
        context,
        id,
        rgba: color.repeat(4),
        width: 2,
        height: 2,
    }
}

#[test]
#[ignore = "gpu: raster frame identity and matching-context ancestor/child fallback"]
fn gpu_raster_frame_context_isolation() {
    use sha2::{Digest, Sha256};
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let (device, queue, adapter) = init_gpu(&runtime).expect("GPU required for raster isolation");
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let mut resources = RenderResources::new(&device, format);
    let targets = [
        new_target(&device, format, SIZE),
        new_target(&device, format, SIZE),
    ];
    let destination = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/parity-review/raster-context/gpu");
    std::fs::create_dir_all(&destination).unwrap();
    let mut evidence = Vec::new();
    let capture = |name: &str,
                   pane: usize,
                   resources: &RenderResources,
                   evidence: &mut Vec<serde_json::Value>| {
        let view = targets[pane].create_view(&Default::default());
        resources.draw_pane(&device, &queue, &view, pane as u32, wgpu::Color::BLACK);
        let pixels = read_target(&device, &queue, &targets[pane], SIZE);
        let center: [u8; 4] = pixels[((SIZE / 2 * SIZE + SIZE / 2) * 4) as usize..][..4]
            .try_into()
            .unwrap();
        assert!(
            pixels
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| *pixel == center),
            "{name}: solid control must cover the whole pane"
        );
        let path = destination.join(format!("{name}.png"));
        image::RgbaImage::from_raw(SIZE, SIZE, pixels.clone())
            .unwrap()
            .save(&path)
            .unwrap();
        evidence.push(serde_json::json!({"name":name,"pane":pane,"center_rgba":center,"sha256":format!("{:x}",Sha256::digest(&pixels))}));
        center
    };
    let a = RasterContext {
        time: Some(1_700_000_041),
        ..Default::default()
    };
    let b = RasterContext {
        time: Some(1_700_000_042),
        ..Default::default()
    };
    let red = [255, 0, 0, 255];
    let blue = [0, 0, 255, 255];
    let green = [0, 255, 0, 255];
    let yellow = [255, 255, 0, 255];
    let black = [0, 0, 0, 255];
    let mut first = callback(0, a);
    first.new_tiles = vec![upload(a, ID, red), upload(b, ID, blue)];
    resources.prepare_pane(&device, &queue, &first);
    resources.prepare_pane(&device, &queue, &callback(1, b));
    assert_eq!(capture("exact-second-a", 0, &resources, &mut evidence), red);
    assert_eq!(
        capture("exact-second-b", 1, &resources, &mut evidence),
        blue
    );
    let mut late = callback(1, b);
    late.new_tiles = vec![upload(a, ID, red)];
    resources.prepare_pane(&device, &queue, &late);
    assert_eq!(
        capture("late-a-delivery-b", 1, &resources, &mut evidence),
        blue
    );
    resources.prepare_pane(&device, &queue, &callback(0, b));
    resources.prepare_pane(&device, &queue, &callback(1, a));
    assert_eq!(
        capture("time-switch-a-to-b", 0, &resources, &mut evidence),
        blue
    );
    assert_eq!(
        capture("cached-return-b-to-a", 1, &resources, &mut evidence),
        red
    );
    resources.prepare_pane(
        &device,
        &queue,
        &callback(0, RasterContext { revision: 1, ..a }),
    );
    assert_eq!(
        capture("different-provider-revision", 0, &resources, &mut evidence),
        black
    );
    // Fresh latest epochs cannot borrow bytes from the previous latest alias.
    let latest = RasterContext {
        latest_epoch: 1,
        ..Default::default()
    };
    let mut latest_upload = callback(0, latest);
    latest_upload.new_tiles = vec![upload(latest, ID, green)];
    resources.prepare_pane(&device, &queue, &latest_upload);
    assert_eq!(
        capture("latest-epoch-one", 0, &resources, &mut evidence),
        green
    );
    resources.prepare_pane(
        &device,
        &queue,
        &callback(
            0,
            RasterContext {
                latest_epoch: 2,
                ..latest
            },
        ),
    );
    assert_eq!(
        capture("latest-epoch-two-missing", 0, &resources, &mut evidence),
        black
    );

    let parent = (5, 7, 12);
    let children: Vec<_> = [(0, 0), (1, 0), (0, 1), (1, 1)]
        .into_iter()
        .map(|(x, y)| (7, 28 + x, 48 + y))
        .collect();
    let mut wrong_pyramid = callback(0, a);
    wrong_pyramid.clear_tiles = true;
    wrong_pyramid.new_tiles = vec![upload(b, parent, blue)];
    wrong_pyramid
        .new_tiles
        .extend(children.iter().map(|id| upload(b, *id, blue)));
    resources.prepare_pane(&device, &queue, &wrong_pyramid);
    assert_eq!(
        capture("foreign-parent-and-children", 0, &resources, &mut evidence),
        black
    );
    let mut correct_parent = callback(0, a);
    correct_parent.new_tiles = vec![upload(a, parent, green)];
    resources.prepare_pane(&device, &queue, &correct_parent);
    assert_eq!(
        capture("matching-parent", 0, &resources, &mut evidence),
        green
    );
    let mut correct_children = callback(0, a);
    correct_children.new_tiles = children.iter().map(|id| upload(a, *id, yellow)).collect();
    resources.prepare_pane(&device, &queue, &correct_children);
    assert_eq!(
        capture("matching-children", 0, &resources, &mut evidence),
        yellow
    );
    let mut retire = callback(0, a);
    retire.drop_tiles = children
        .iter()
        .chain(std::iter::once(&parent))
        .map(|id| (style(), a, *id))
        .collect();
    resources.prepare_pane(&device, &queue, &retire);
    resources.prepare_pane(&device, &queue, &callback(1, b));
    assert_eq!(
        capture("retired-context", 0, &resources, &mut evidence),
        black
    );
    assert_eq!(
        capture("surviving-pyramid-context", 1, &resources, &mut evidence),
        blue
    );
    std::fs::write(
        destination.join("verification.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"adapter":adapter.get_info().name,"captures":evidence}),
        )
        .unwrap(),
    )
    .unwrap();
}
