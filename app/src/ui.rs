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
    /// Path typed into the Save/Load text field; defaulted from the video path on first load.
    pub project_path_text: String,
    /// Set by the Save/Load buttons; the app loop consumes and clears these each redraw.
    pub save_requested: bool,
    pub load_requested: bool,
    pub status_message: Option<String>,
}

pub fn draw(ui: &mut egui::Ui, state: &mut UiState) {
    let screen = ui.max_rect();
    draw_calibration_handles(ui, screen, &mut state.calibration);
    draw_sync_and_project_window(ui, state);
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

fn format_timecode(seconds: f64) -> String {
    let total_ms = (seconds.max(0.0) * 1000.0).round() as i64;
    let minutes = total_ms / 60_000;
    let secs = (total_ms / 1000) % 60;
    let millis = total_ms % 1000;
    format!("{minutes:02}:{secs:02}.{millis:03}")
}
