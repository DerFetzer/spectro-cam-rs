use egui::{Context, Slider};

/// The allowed range of number of camera frames to average over when computing the spectrum.
const SPECTRUM_BUFFER_SIZE_RANGE: std::ops::RangeInclusive<usize> = 1..=100;

const SPECTRUM_FILTER_CUTOFF_RANGE: std::ops::RangeInclusive<f32> = 0.001..=1.0;

pub struct PostProcessingWindow {}

impl PostProcessingWindow {
    pub fn new() -> Self {
        Self {}
    }

    pub fn update(&mut self, ctx: &Context, config: &mut crate::config::SpectrometerConfig) {
        egui::Window::new("Postprocessing")
            .open(&mut config.view_config.show_postprocessing_window)
            .show(ctx, |ui| {
                ui.add(
                    Slider::new(
                        &mut config.postprocessing_config.spectrum_buffer_size,
                        SPECTRUM_BUFFER_SIZE_RANGE,
                    )
                    .text("Averaging Buffer Size"),
                );
                ui.separator();
                ui.horizontal(|ui| {
                    ui.checkbox(
                        &mut config.postprocessing_config.spectrum_filter_active,
                        "Low-Pass Filter",
                    );
                    ui.add_enabled(
                        config.postprocessing_config.spectrum_filter_active,
                        Slider::new(
                            &mut config.postprocessing_config.spectrum_filter_cutoff,
                            SPECTRUM_FILTER_CUTOFF_RANGE,
                        )
                        .logarithmic(true)
                        .text("Cutoff"),
                    );
                });
                ui.separator();
                ui.add_enabled(
                    config.reference_config.reference.is_some(),
                    Slider::new(&mut config.reference_config.scale, 0.001..=100.)
                        .logarithmic(true)
                        .text("Reference Scale"),
                );
                ui.separator();
                ui.horizontal(|ui| {
                    ui.checkbox(&mut config.view_config.draw_peaks, "Show Peaks");
                    ui.checkbox(&mut config.view_config.draw_dips, "Show Dips");
                });
                ui.add(
                    Slider::new(&mut config.view_config.peaks_dips_find_window, 1..=200)
                        .text("Peaks/Dips Find Window"),
                );
                ui.add(
                    Slider::new(&mut config.view_config.peaks_dips_unique_window, 1.0..=200.)
                        .text("Peaks/Dips Filter Window"),
                );
                ui.separator();
                ui.checkbox(
                    &mut config.view_config.draw_color_polygons,
                    "Show colors under spectrum",
                );
            });
    }
}
