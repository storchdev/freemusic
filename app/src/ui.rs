pub struct UiState {
    pub playing: bool,
    pub position_seconds: f64,
    pub duration_seconds: f64,
    /// Set by the UI when the user drags the scrub bar; the app loop consumes and clears it.
    pub seek_request: Option<f64>,
    /// True while a file is being dragged over the window; drives the drop-zone overlay.
    pub dropping: bool,
    pub midi_name: Option<String>,
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
}

pub fn draw(ui: &mut egui::Ui, state: &mut UiState) {
    let screen = ui.max_rect();
    draw_calibration_handles(ui, screen, &mut state.calibration);
    draw_crop_handles(ui, screen, &mut state.transform);
    draw_sync_and_project_window(ui, state);
    draw_transform_window(ui, &mut state.transform);
    draw_export_window(ui, state);
    if state.dropping {
        draw_drop_overlay(ui, screen);
    }

    egui::Panel::bottom("transport").show(ui, |ui| {
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

            let mut position = state.position_seconds;
            let slider = ui.add(
                egui::Slider::new(&mut position, 0.0..=state.duration_seconds.max(0.001))
                    .show_value(false),
            );
            if slider.changed() {
                state.seek_request = Some(position);
            }

            if let Some(name) = &state.midi_name {
                ui.separator();
                ui.label(format!("MIDI: {name}"));
            }
        });
        ui.add_space(6.0);
    });
}

fn draw_drop_overlay(ui: &mut egui::Ui, screen: egui::Rect) {
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

/// Minimum gap kept between the left/right calibration handles, as a fraction of window width
/// — keeps them from being dragged past each other into a zero/negative-width keyboard.
const CALIBRATION_MIN_GAP: f32 = 0.05;

/// Draws two draggable vertical guides over the video preview marking the left/right edges of
/// the keyboard visible in the footage, updating `calibration` in place as they're dragged.
/// Left off the bottom ~60px so they don't fight the transport bar for drag input.
fn draw_calibration_handles(
    ui: &egui::Ui,
    screen: egui::Rect,
    calibration: &mut project::KeyboardCalibration,
) {
    let handles_bottom = (screen.bottom() - 60.0).max(screen.top());
    let stroke = egui::Stroke::new(3.0, egui::Color32::from_rgb(255, 209, 0));

    let left_x = screen.left() + screen.width() * calibration.left_fraction;
    let left_rect = egui::Rect::from_min_max(
        egui::pos2(left_x - 6.0, screen.top()),
        egui::pos2(left_x + 6.0, handles_bottom),
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
        egui::pos2(right_x + 6.0, handles_bottom),
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
            egui::pos2(left_x, handles_bottom),
        ],
        stroke,
    );
    painter.line_segment(
        [
            egui::pos2(right_x, screen.top()),
            egui::pos2(right_x, handles_bottom),
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

/// Minimum gap kept between opposing crop edges, as a fraction of frame width/height — same
/// purpose as `CALIBRATION_MIN_GAP`, keeps a drag from collapsing the crop to zero size.
const CROP_MIN_GAP: f32 = 0.1;

/// Draws four draggable guides (a rectangle) over the video preview marking the crop region,
/// in a different color from the keyboard calibration handles so the two don't get confused
/// when both are visible. Same `Sense::drag()` + accumulated `drag_delta()` pattern as
/// `draw_calibration_handles`, extended to both axes.
fn draw_crop_handles(ui: &egui::Ui, screen: egui::Rect, transform: &mut project::VideoTransform) {
    let handles_bottom = (screen.bottom() - 60.0).max(screen.top());
    let inner = egui::Rect::from_min_max(
        egui::pos2(screen.left(), screen.top()),
        egui::pos2(screen.right(), handles_bottom),
    );
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

/// Floating window with brightness/rotation/tilt sliders and a crop readout — everything from
/// milestone 4 that isn't a drag handle directly on the preview. A separate window from
/// "Sync & Project" (default-positioned beside it) rather than folded into it, since the two
/// control unrelated things and the combined window was already getting long.
fn draw_transform_window(ui: &egui::Ui, transform: &mut project::VideoTransform) {
    egui::Window::new("Video Transform")
        .default_pos(egui::pos2(340.0, 20.0))
        .resizable(false)
        .show(ui.ctx(), |ui| {
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
            // Same min-gap clamp as the drag handles, so a slider can't collapse the crop to
            // zero size either.
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
        });
}

/// Floating (movable, so it never permanently blocks the video preview) panel holding the sync
/// offset control and project save/load — everything from milestone 3 that isn't a drag handle
/// directly on the preview.
fn draw_sync_and_project_window(ui: &egui::Ui, state: &mut UiState) {
    egui::Window::new("Sync & Project")
        .default_pos(egui::pos2(20.0, 20.0))
        .resizable(false)
        .show(ui.ctx(), |ui| {
            ui.horizontal(|ui| {
                ui.label("Sync offset (s):");
                ui.add(egui::DragValue::new(&mut state.sync_offset_seconds).speed(0.01));
            });
            ui.horizontal(|ui| {
                ui.label("Keyboard left:");
                ui.add(egui::Slider::new(
                    &mut state.calibration.left_fraction,
                    0.0..=1.0,
                ));
            });
            ui.horizontal(|ui| {
                ui.label("Keyboard right:");
                ui.add(egui::Slider::new(
                    &mut state.calibration.right_fraction,
                    0.0..=1.0,
                ));
            });
            // Same min-gap clamp as the drag handles (`draw_calibration_handles`), so a slider
            // can't collapse the keyboard span to zero width either.
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

            ui.separator();
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
        });
}

/// Floating window driving MP4 export: output path, fps, and an Export button that becomes a
/// progress bar + Cancel once `export_progress` is `Some` (set by the app loop while a
/// background export thread is running — see `main.rs`'s `AppState::start_export`).
fn draw_export_window(ui: &egui::Ui, state: &mut UiState) {
    egui::Window::new("Export")
        .default_pos(egui::pos2(660.0, 20.0))
        .resizable(false)
        .show(ui.ctx(), |ui| {
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
        });
}

fn format_timecode(seconds: f64) -> String {
    let total_ms = (seconds.max(0.0) * 1000.0).round() as i64;
    let minutes = total_ms / 60_000;
    let secs = (total_ms / 1000) % 60;
    let millis = total_ms % 1000;
    format!("{minutes:02}:{secs:02}.{millis:03}")
}
