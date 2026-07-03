pub struct UiState {
    pub playing: bool,
    pub position_seconds: f64,
    pub duration_seconds: f64,
    /// Set by the UI when the user drags the scrub bar; the app loop consumes and clears it.
    pub seek_request: Option<f64>,
    /// True while a file is being dragged over the window; drives the drop-zone overlay.
    pub dropping: bool,
    pub midi_name: Option<String>,
}

pub fn draw(ui: &mut egui::Ui, state: &mut UiState) {
    let screen = ui.max_rect();
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

fn format_timecode(seconds: f64) -> String {
    let total_ms = (seconds.max(0.0) * 1000.0).round() as i64;
    let minutes = total_ms / 60_000;
    let secs = (total_ms / 1000) % 60;
    let millis = total_ms % 1000;
    format!("{minutes:02}:{secs:02}.{millis:03}")
}
