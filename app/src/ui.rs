pub struct UiState {
    pub playing: bool,
    pub position_seconds: f64,
    pub duration_seconds: f64,
    /// Set by the UI when the user drags/clicks the timeline; the app loop consumes and clears
    /// it.
    pub seek_request: Option<f64>,
    /// True while a file is being dragged over the window; drives the drop-zone overlay.
    pub dropping: bool,
    pub midi_name: Option<String>,
    /// Sorted note onset times in seconds, mirrored from `Compositor::note_start_times` each
    /// time a MIDI file loads — used by the bottom timeline's note-density strip.
    pub midi_note_times: Vec<f32>,
    /// `midi_time = position_seconds - sync_offset_seconds`; video is always the master clock,
    /// dragging this only moves where notes render relative to it.
    pub sync_offset_seconds: f64,
    pub calibration: project::KeyboardCalibration,
    pub transform: project::VideoTransform,
    /// Path typed into the Save/Load text field; defaulted from the video path on first load.
    pub project_path_text: String,
    /// Set by the Save/Load buttons; the app loop consumes and clears these each redraw.
    pub save_requested: bool,
    pub load_requested: bool,
    /// Set by the Project tab's "Open Video…"/"Open MIDI…" buttons; the app loop consumes and
    /// clears these each redraw, popping a native `rfd` file picker and loading whatever the
    /// user chose (a no-op if they cancel).
    pub open_video_requested: bool,
    pub open_midi_requested: bool,
    pub status_message: Option<String>,
    /// Path typed into the Export text field; defaulted from the video path on first load.
    pub export_path_text: String,
    pub export_fps: u32,
    /// Set by the Export/Cancel buttons; the app loop consumes and clears these each redraw.
    pub export_requested: bool,
    pub export_cancel_requested: bool,
    /// `(frames done, total frames)` while an export is running, driven by the app loop
    /// draining `export::Progress` off the background thread's channel; `None` otherwise.
    pub export_progress: Option<(u64, u64)>,
    pub export_message: Option<String>,
    /// Which side-panel tab is currently showing.
    pub active_tab: Tab,
    /// Egui-registered id of the offscreen preview texture `AppState` renders the compositor
    /// into; displayed via `egui::Image` in the central panel.
    pub preview_texture_id: egui::TextureId,
    /// Pixel size of that texture — the compositor's canvas, decoupled from window size (see
    /// CLAUDE.md's milestone 6c notes). Used here only to compute the aspect-fit display rect.
    pub canvas_size: (u32, u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Project,
    Keyboard,
    Transform,
    Export,
}

pub fn draw(ui: &mut egui::Ui, state: &mut UiState) {
    draw_side_panel(ui, state);
    draw_timeline_panel(ui, state);

    egui::CentralPanel::default().show(ui, |ui| {
        let image_rect = fit_rect(ui.available_rect_before_wrap(), canvas_aspect(state));
        ui.put(
            image_rect,
            egui::Image::new((state.preview_texture_id, image_rect.size()))
                .fit_to_exact_size(image_rect.size()),
        );
        draw_calibration_handles(ui, image_rect, &mut state.calibration);
        draw_crop_handles(ui, image_rect, &mut state.transform);
    });

    if state.dropping {
        draw_drop_overlay(ui, ui.max_rect());
    }
}

fn canvas_aspect(state: &UiState) -> f32 {
    let (w, h) = state.canvas_size;
    if h == 0 {
        1.0
    } else {
        w as f32 / h as f32
    }
}

/// Centers a rect of the given aspect ratio inside `container`, contain-fit (letterboxed, never
/// cropped) — this is the rect the preview image is drawn into, and the rect all the
/// calibration/crop drag handles hit-test and draw against (replacing the old assumption that
/// the video filled the whole window, back when it was painted directly onto the swapchain).
fn fit_rect(container: egui::Rect, aspect: f32) -> egui::Rect {
    let container_w = container.width().max(1.0);
    let container_h = container.height().max(1.0);
    let container_aspect = container_w / container_h;
    let size = if aspect > container_aspect {
        egui::vec2(container_w, container_w / aspect)
    } else {
        egui::vec2(container_h * aspect, container_h)
    };
    egui::Rect::from_center_size(container.center(), size)
}

/// Hand-rolled tab strip + content for the persistent side panel — replaces the four floating
/// windows milestones 3-5 used (Sync & Project / Video Transform / Export, plus a keyboard
/// calibration readout folded into the first). A real `egui::SidePanel`-equivalent (`egui::
/// Panel::left`) only became viable once the video moved to a shrinkable `egui::Image` in the
/// central panel instead of being painted directly under floating windows — see CLAUDE.md.
fn draw_side_panel(ui: &mut egui::Ui, state: &mut UiState) {
    egui::Panel::left("side_panel")
        .default_size(280.0)
        .min_size(220.0)
        .max_size(420.0)
        .show(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                tab_button(ui, state, Tab::Project, "Project");
                tab_button(ui, state, Tab::Keyboard, "Keyboard");
                tab_button(ui, state, Tab::Transform, "Transform");
                tab_button(ui, state, Tab::Export, "Export");
            });
            ui.separator();
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| match state.active_tab {
                    Tab::Project => draw_project_tab(ui, state),
                    Tab::Keyboard => draw_keyboard_tab(ui, state),
                    Tab::Transform => draw_transform_tab(ui, &mut state.transform),
                    Tab::Export => draw_export_tab(ui, state),
                });
        });
}

fn tab_button(ui: &mut egui::Ui, state: &mut UiState, tab: Tab, label: &str) {
    if ui
        .selectable_label(state.active_tab == tab, label)
        .clicked()
    {
        state.active_tab = tab;
    }
}

/// Minimum gap kept between the left/right calibration handles, as a fraction of the preview
/// image width — keeps them from being dragged past each other into a zero/negative-width
/// keyboard.
const CALIBRATION_MIN_GAP: f32 = 0.05;

fn draw_keyboard_tab(ui: &mut egui::Ui, state: &mut UiState) {
    ui.heading("Keyboard calibration");
    ui.label("Drag the yellow guides on the preview, or use the sliders below.");
    ui.horizontal(|ui| {
        ui.label("Left:");
        ui.add(egui::Slider::new(
            &mut state.calibration.left_fraction,
            0.0..=1.0,
        ));
    });
    ui.horizontal(|ui| {
        ui.label("Right:");
        ui.add(egui::Slider::new(
            &mut state.calibration.right_fraction,
            0.0..=1.0,
        ));
    });
    // Same min-gap clamp as the drag handles, so a slider can't collapse the keyboard span to
    // zero width either.
    state.calibration.right_fraction = state
        .calibration
        .right_fraction
        .clamp(state.calibration.left_fraction + CALIBRATION_MIN_GAP, 1.0);
    state.calibration.left_fraction = state
        .calibration
        .left_fraction
        .clamp(0.0, state.calibration.right_fraction - CALIBRATION_MIN_GAP);
    ui.label(format!(
        "Keyboard: {:.0}%\u{2013}{:.0}% of width",
        state.calibration.left_fraction * 100.0,
        state.calibration.right_fraction * 100.0,
    ));
    if ui.button("Reset calibration").clicked() {
        state.calibration = project::KeyboardCalibration::default();
    }
}

fn draw_project_tab(ui: &mut egui::Ui, state: &mut UiState) {
    ui.heading("Media");
    ui.horizontal(|ui| {
        if ui.button("Open Video…").clicked() {
            state.open_video_requested = true;
        }
        if ui.button("Open MIDI…").clicked() {
            state.open_midi_requested = true;
        }
    });

    ui.separator();
    ui.heading("Sync");
    ui.horizontal(|ui| {
        ui.label("Sync offset (s):");
        ui.add(egui::DragValue::new(&mut state.sync_offset_seconds).speed(0.01));
    });

    ui.separator();
    ui.heading("Project file");
    ui.label("Project file:");
    ui.text_edit_singleline(&mut state.project_path_text);
    ui.horizontal(|ui| {
        if ui.button("Save").clicked() {
            state.save_requested = true;
        }
        if ui.button("Load").clicked() {
            state.load_requested = true;
        }
    });
    if let Some(message) = &state.status_message {
        ui.label(message);
    }
}

/// Minimum gap kept between opposing crop edges, as a fraction of frame width/height — same
/// purpose as `CALIBRATION_MIN_GAP`, keeps a drag from collapsing the crop to zero size.
const CROP_MIN_GAP: f32 = 0.1;

fn draw_transform_tab(ui: &mut egui::Ui, transform: &mut project::VideoTransform) {
    ui.heading("Video Transform");
    ui.horizontal(|ui| {
        ui.label("Brightness:");
        ui.add(egui::Slider::new(&mut transform.brightness, 0.0..=2.0));
    });
    ui.horizontal(|ui| {
        ui.label("Scale (zoom):");
        ui.add(egui::Slider::new(&mut transform.scale, 0.2..=3.0));
    });
    ui.horizontal(|ui| {
        ui.label("Rotation (deg):");
        ui.add(egui::Slider::new(
            &mut transform.rotation_degrees,
            -45.0..=45.0,
        ));
    });
    ui.horizontal(|ui| {
        ui.label("Tilt X:");
        ui.add(egui::Slider::new(&mut transform.tilt_x, -0.3..=0.3));
    });
    ui.horizontal(|ui| {
        ui.label("Tilt Y:");
        ui.add(egui::Slider::new(&mut transform.tilt_y, -0.3..=0.3));
    });
    ui.horizontal(|ui| {
        ui.label("Translate X:");
        ui.add(egui::Slider::new(&mut transform.translate_x, -1.0..=1.0));
    });
    ui.horizontal(|ui| {
        ui.label("Translate Y:");
        ui.add(egui::Slider::new(&mut transform.translate_y, -1.0..=1.0));
    });

    ui.separator();
    ui.label("Crop (also draggable on the preview):");
    ui.horizontal(|ui| {
        ui.label("Left:");
        ui.add(egui::Slider::new(&mut transform.crop_left, 0.0..=1.0));
    });
    ui.horizontal(|ui| {
        ui.label("Right:");
        ui.add(egui::Slider::new(&mut transform.crop_right, 0.0..=1.0));
    });
    ui.horizontal(|ui| {
        ui.label("Top:");
        ui.add(egui::Slider::new(&mut transform.crop_top, 0.0..=1.0));
    });
    ui.horizontal(|ui| {
        ui.label("Bottom:");
        ui.add(egui::Slider::new(&mut transform.crop_bottom, 0.0..=1.0));
    });
    // Same min-gap clamp as the drag handles, so a slider can't collapse the crop to zero size
    // either.
    transform.crop_right = transform
        .crop_right
        .clamp(transform.crop_left + CROP_MIN_GAP, 1.0);
    transform.crop_left = transform
        .crop_left
        .clamp(0.0, transform.crop_right - CROP_MIN_GAP);
    transform.crop_bottom = transform
        .crop_bottom
        .clamp(transform.crop_top + CROP_MIN_GAP, 1.0);
    transform.crop_top = transform
        .crop_top
        .clamp(0.0, transform.crop_bottom - CROP_MIN_GAP);

    if ui.button("Reset transform").clicked() {
        *transform = project::VideoTransform::default();
    }
}

fn draw_export_tab(ui: &mut egui::Ui, state: &mut UiState) {
    ui.heading("Export");
    ui.horizontal(|ui| {
        ui.label("Output file:");
        ui.text_edit_singleline(&mut state.export_path_text);
    });
    ui.horizontal(|ui| {
        ui.label("FPS:");
        ui.add(egui::DragValue::new(&mut state.export_fps).range(1..=120));
    });

    if let Some((done, total)) = state.export_progress {
        let fraction = if total > 0 {
            done as f32 / total as f32
        } else {
            0.0
        };
        ui.add(
            egui::ProgressBar::new(fraction)
                .show_percentage()
                .text(format!("{done} / {total} frames")),
        );
        if ui.button("Cancel").clicked() {
            state.export_cancel_requested = true;
        }
    } else if ui.button("Export").clicked() {
        state.export_requested = true;
    }

    if let Some(message) = &state.export_message {
        ui.label(message);
    }
}

/// Height of the custom-painted ruler/playhead/density strip, not counting the transport
/// controls row above it.
const TIMELINE_STRIP_HEIGHT: f32 = 40.0;

/// Bottom transport bar: play/pause, timecode, and a custom-painted timeline (time ruler,
/// draggable/clickable playhead, note-density strip) — replaces the plain `egui::Slider`
/// scrubber milestones 1-5 used, which had no room to show song structure while scrubbing.
fn draw_timeline_panel(ui: &mut egui::Ui, state: &mut UiState) {
    egui::Panel::bottom("timeline")
        .default_size(96.0)
        .resizable(false)
        .show(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let play_label = if state.playing { "Pause" } else { "Play" };
                if ui.button(play_label).clicked() {
                    state.playing = !state.playing;
                }
                ui.label(format!(
                    "{} / {}",
                    format_timecode(state.position_seconds),
                    format_timecode(state.duration_seconds)
                ));
                if let Some(name) = &state.midi_name {
                    ui.separator();
                    ui.label(format!("MIDI: {name}"));
                }
            });
            ui.add_space(4.0);
            draw_timeline_scrubber(ui, state);
            ui.add_space(6.0);
        });
}

fn draw_timeline_scrubber(ui: &mut egui::Ui, state: &mut UiState) {
    let desired_size = egui::vec2(ui.available_width(), TIMELINE_STRIP_HEIGHT);
    let (rect, response) = ui.allocate_exact_size(desired_size, egui::Sense::click_and_drag());
    let duration = state.duration_seconds.max(0.001);

    if response.clicked() || response.dragged() {
        if let Some(pos) = response.interact_pointer_pos() {
            let fraction = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
            state.seek_request = Some(fraction as f64 * duration);
        }
    }

    let painter = ui.painter();
    painter.rect_filled(rect, 3.0, egui::Color32::from_gray(35));

    draw_note_density(painter, rect, &state.midi_note_times, duration);
    draw_time_ruler(painter, rect, duration);

    let playhead_x =
        rect.left() + (state.position_seconds / duration).clamp(0.0, 1.0) as f32 * rect.width();
    painter.line_segment(
        [
            egui::pos2(playhead_x, rect.top()),
            egui::pos2(playhead_x, rect.bottom()),
        ],
        egui::Stroke::new(2.0, egui::Color32::from_rgb(255, 90, 90)),
    );
}

/// Buckets note onset times into columns spanning the timeline's width and draws each as a
/// short vertical bar sized by relative density — cheap (the MIDI is already parsed once at
/// load time) and gives a rough sense of song structure while scrubbing, without needing to
/// actually render note content in miniature.
fn draw_note_density(painter: &egui::Painter, rect: egui::Rect, note_times: &[f32], duration: f64) {
    if note_times.is_empty() {
        return;
    }
    const BUCKETS: usize = 240;
    let mut counts = [0u32; BUCKETS];
    for &t in note_times {
        let fraction = (t as f64 / duration).clamp(0.0, 0.999_999);
        counts[(fraction * BUCKETS as f64) as usize] += 1;
    }
    let max_count = counts.iter().copied().max().unwrap_or(1).max(1);
    let bucket_width = rect.width() / BUCKETS as f32;
    let strip_bottom = rect.bottom() - 3.0;
    let strip_max_height = rect.height() - 16.0;
    for (i, &count) in counts.iter().enumerate() {
        if count == 0 {
            continue;
        }
        let height = (count as f32 / max_count as f32) * strip_max_height;
        let x = rect.left() + i as f32 * bucket_width;
        let bar = egui::Rect::from_min_max(
            egui::pos2(x, strip_bottom - height),
            egui::pos2(x + bucket_width.max(1.0), strip_bottom),
        );
        painter.rect_filled(bar, 0.0, egui::Color32::from_rgb(80, 150, 210));
    }
}

/// Draws tick marks + timecode labels at a duration-appropriate interval (aiming for roughly
/// 6-12 ticks regardless of clip length).
fn draw_time_ruler(painter: &egui::Painter, rect: egui::Rect, duration: f64) {
    let interval = ruler_tick_interval(duration);
    let mut t = 0.0;
    while t <= duration {
        let x = rect.left() + (t / duration) as f32 * rect.width();
        painter.line_segment(
            [egui::pos2(x, rect.top()), egui::pos2(x, rect.top() + 5.0)],
            egui::Stroke::new(1.0, egui::Color32::from_gray(150)),
        );
        painter.text(
            egui::pos2(x + 2.0, rect.top()),
            egui::Align2::LEFT_TOP,
            format_timecode(t),
            egui::FontId::proportional(9.0),
            egui::Color32::from_gray(190),
        );
        t += interval;
    }
}

fn ruler_tick_interval(duration: f64) -> f64 {
    const STEPS: [f64; 10] = [1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0];
    let target = duration / 10.0;
    STEPS
        .into_iter()
        .find(|&step| step >= target)
        .unwrap_or(600.0)
}

fn draw_drop_overlay(ui: &egui::Ui, screen: egui::Rect) {
    egui::Area::new(egui::Id::new("drop_overlay"))
        .fixed_pos(egui::Pos2::ZERO)
        .order(egui::Order::Foreground)
        .show(ui.ctx(), |ui| {
            ui.painter()
                .rect_filled(screen, 0.0, egui::Color32::from_black_alpha(170));
            ui.painter().text(
                screen.center(),
                egui::Align2::CENTER_CENTER,
                "Drop a video or MIDI (.mid) file",
                egui::FontId::proportional(28.0),
                egui::Color32::WHITE,
            );
        });
}

/// Draws two draggable vertical guides over the preview image marking the left/right edges of
/// the keyboard visible in the footage, updating `calibration` in place as they're dragged.
/// `screen` is the preview image's actual on-screen rect (aspect-fit inside the central panel,
/// see `fit_rect`) — not the whole window, since milestone 6c moved the video off the raw
/// swapchain and into a shrinkable `egui::Image`.
fn draw_calibration_handles(
    ui: &egui::Ui,
    screen: egui::Rect,
    calibration: &mut project::KeyboardCalibration,
) {
    let stroke = egui::Stroke::new(3.0, egui::Color32::from_rgb(255, 209, 0));

    let left_x = screen.left() + screen.width() * calibration.left_fraction;
    let left_rect = egui::Rect::from_min_max(
        egui::pos2(left_x - 6.0, screen.top()),
        egui::pos2(left_x + 6.0, screen.bottom()),
    );
    let left_response = ui.interact(
        left_rect,
        egui::Id::new("calibration_left_handle"),
        egui::Sense::drag(),
    );
    if left_response.dragged() {
        let delta = left_response.drag_delta().x / screen.width();
        calibration.left_fraction = (calibration.left_fraction + delta)
            .clamp(0.0, calibration.right_fraction - CALIBRATION_MIN_GAP);
    }

    let right_x = screen.left() + screen.width() * calibration.right_fraction;
    let right_rect = egui::Rect::from_min_max(
        egui::pos2(right_x - 6.0, screen.top()),
        egui::pos2(right_x + 6.0, screen.bottom()),
    );
    let right_response = ui.interact(
        right_rect,
        egui::Id::new("calibration_right_handle"),
        egui::Sense::drag(),
    );
    if right_response.dragged() {
        let delta = right_response.drag_delta().x / screen.width();
        calibration.right_fraction = (calibration.right_fraction + delta)
            .clamp(calibration.left_fraction + CALIBRATION_MIN_GAP, 1.0);
    }

    let painter = ui.painter();
    painter.line_segment(
        [
            egui::pos2(left_x, screen.top()),
            egui::pos2(left_x, screen.bottom()),
        ],
        stroke,
    );
    painter.line_segment(
        [
            egui::pos2(right_x, screen.top()),
            egui::pos2(right_x, screen.bottom()),
        ],
        stroke,
    );
    painter.text(
        egui::pos2(left_x, screen.top() + 4.0),
        egui::Align2::CENTER_TOP,
        "key L",
        egui::FontId::proportional(12.0),
        stroke.color,
    );
    painter.text(
        egui::pos2(right_x, screen.top() + 4.0),
        egui::Align2::CENTER_TOP,
        "key R",
        egui::FontId::proportional(12.0),
        stroke.color,
    );
}

/// Draws four draggable guides (a rectangle) over the preview image marking the crop region,
/// in a different color from the keyboard calibration handles so the two don't get confused
/// when both are visible. Same `Sense::drag()` + accumulated `drag_delta()` pattern as
/// `draw_calibration_handles`, extended to both axes, and against the same aspect-fit `screen`
/// rect.
fn draw_crop_handles(ui: &egui::Ui, screen: egui::Rect, transform: &mut project::VideoTransform) {
    let inner = screen;
    let stroke = egui::Stroke::new(3.0, egui::Color32::from_rgb(0, 220, 220));

    let left_x = inner.left() + inner.width() * transform.crop_left;
    let right_x = inner.left() + inner.width() * transform.crop_right;
    let top_y = inner.top() + inner.height() * transform.crop_top;
    let bottom_y = inner.top() + inner.height() * transform.crop_bottom;

    let left_rect = egui::Rect::from_min_max(
        egui::pos2(left_x - 6.0, inner.top()),
        egui::pos2(left_x + 6.0, inner.bottom()),
    );
    let left_response = ui.interact(
        left_rect,
        egui::Id::new("crop_left_handle"),
        egui::Sense::drag(),
    );
    if left_response.dragged() {
        let delta = left_response.drag_delta().x / inner.width();
        transform.crop_left =
            (transform.crop_left + delta).clamp(0.0, transform.crop_right - CROP_MIN_GAP);
    }

    let right_rect = egui::Rect::from_min_max(
        egui::pos2(right_x - 6.0, inner.top()),
        egui::pos2(right_x + 6.0, inner.bottom()),
    );
    let right_response = ui.interact(
        right_rect,
        egui::Id::new("crop_right_handle"),
        egui::Sense::drag(),
    );
    if right_response.dragged() {
        let delta = right_response.drag_delta().x / inner.width();
        transform.crop_right =
            (transform.crop_right + delta).clamp(transform.crop_left + CROP_MIN_GAP, 1.0);
    }

    let top_rect = egui::Rect::from_min_max(
        egui::pos2(inner.left(), top_y - 6.0),
        egui::pos2(inner.right(), top_y + 6.0),
    );
    let top_response = ui.interact(
        top_rect,
        egui::Id::new("crop_top_handle"),
        egui::Sense::drag(),
    );
    if top_response.dragged() {
        let delta = top_response.drag_delta().y / inner.height();
        transform.crop_top =
            (transform.crop_top + delta).clamp(0.0, transform.crop_bottom - CROP_MIN_GAP);
    }

    let bottom_rect = egui::Rect::from_min_max(
        egui::pos2(inner.left(), bottom_y - 6.0),
        egui::pos2(inner.right(), bottom_y + 6.0),
    );
    let bottom_response = ui.interact(
        bottom_rect,
        egui::Id::new("crop_bottom_handle"),
        egui::Sense::drag(),
    );
    if bottom_response.dragged() {
        let delta = bottom_response.drag_delta().y / inner.height();
        transform.crop_bottom =
            (transform.crop_bottom + delta).clamp(transform.crop_top + CROP_MIN_GAP, 1.0);
    }

    let crop_rect =
        egui::Rect::from_min_max(egui::pos2(left_x, top_y), egui::pos2(right_x, bottom_y));
    ui.painter()
        .rect_stroke(crop_rect, 0.0, stroke, egui::StrokeKind::Outside);
}

fn format_timecode(seconds: f64) -> String {
    let total_ms = (seconds.max(0.0) * 1000.0).round() as i64;
    let minutes = total_ms / 60_000;
    let secs = (total_ms / 1000) % 60;
    let millis = total_ms % 1000;
    format!("{minutes:02}:{secs:02}.{millis:03}")
}
