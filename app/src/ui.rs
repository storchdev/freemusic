pub struct UiState {
    pub playing: bool,
    pub position_seconds: f64,
    pub duration_seconds: f64,
    /// Set by the UI when the user drags the scrub bar; the app loop consumes and clears it.
    pub seek_request: Option<f64>,
}

pub fn draw(ui: &mut egui::Ui, state: &mut UiState) {
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
        });
        ui.add_space(6.0);
    });
}

fn format_timecode(seconds: f64) -> String {
    let total_ms = (seconds.max(0.0) * 1000.0).round() as i64;
    let minutes = total_ms / 60_000;
    let secs = (total_ms / 1000) % 60;
    let millis = total_ms % 1000;
    format!("{minutes:02}:{secs:02}.{millis:03}")
}
