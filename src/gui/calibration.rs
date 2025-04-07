use crate::config::{GainPresets, Linearize};
use crate::spectrum;
use egui::{Button, ComboBox, Context, Slider};

pub struct CalibrationWindow {}

impl CalibrationWindow {
    pub fn new() -> Self {
        Self {}
    }

    pub fn update(
        &mut self,
        ctx: &Context,
        config: &mut crate::config::SpectrometerConfig,
        spectrum_container: &mut spectrum::SpectrumContainer,
    ) {
        egui::Window::new("Calibration")
            .open(&mut config.view_config.show_calibration_window)
            .show(ctx, |ui| {
                ui.add(
                    Slider::new(
                        &mut config.spectrum_calibration.low.wavelength,
                        200..=config.spectrum_calibration.high.wavelength - 1,
                    )
                    .text("Low Wavelength"),
                );
                ui.add(
                    Slider::new(
                        &mut config.spectrum_calibration.low.index,
                        0..=config.spectrum_calibration.high.index - 1,
                    )
                    .text("Low Index"),
                );

                ui.add(
                    Slider::new(
                        &mut config.spectrum_calibration.high.wavelength,
                        (config.spectrum_calibration.low.wavelength + 1)..=2000,
                    )
                    .text("High Wavelength"),
                );
                ui.add(
                    Slider::new(
                        &mut config.spectrum_calibration.high.index,
                        (config.spectrum_calibration.low.index + 1)
                            ..=config.image_config.window.size.x as usize,
                    )
                    .text("High Index"),
                );
                ui.separator();
                ComboBox::from_label("Linearize")
                    .selected_text(config.spectrum_calibration.linearize.to_string())
                    .show_ui(ui, |ui| {
                        let mut changed = false;
                        changed |= ui
                            .selectable_value(
                                &mut config.spectrum_calibration.linearize,
                                Linearize::Off,
                                Linearize::Off.to_string(),
                            )
                            .changed();
                        changed |= ui
                            .selectable_value(
                                &mut config.spectrum_calibration.linearize,
                                Linearize::Rec601,
                                Linearize::Rec601.to_string(),
                            )
                            .changed();
                        changed |= ui
                            .selectable_value(
                                &mut config.spectrum_calibration.linearize,
                                Linearize::Rec709,
                                Linearize::Rec709.to_string(),
                            )
                            .changed();
                        changed |= ui
                            .selectable_value(
                                &mut config.spectrum_calibration.linearize,
                                Linearize::SRgb,
                                Linearize::SRgb.to_string(),
                            )
                            .changed();

                        // Clear buffer if value changed
                        if changed {
                            spectrum_container.clear_buffer()
                        };
                    });
                ui.add(
                    Slider::new(&mut config.spectrum_calibration.gain_r, 0.0..=10.).text("Gain R"),
                );
                ui.add(
                    Slider::new(&mut config.spectrum_calibration.gain_g, 0.0..=10.).text("Gain G"),
                );
                ui.add(
                    Slider::new(&mut config.spectrum_calibration.gain_b, 0.0..=10.).text("Gain B"),
                );

                ui.horizontal(|ui| {
                    let unity_button = ui.button(GainPresets::Unity.to_string());
                    if unity_button.clicked() {
                        config
                            .spectrum_calibration
                            .set_gain_preset(GainPresets::Unity);
                    }
                    let srgb_button = ui.button(GainPresets::SRgb.to_string());
                    if srgb_button.clicked() {
                        config
                            .spectrum_calibration
                            .set_gain_preset(GainPresets::SRgb);
                    }
                    let rec601_button = ui.button(GainPresets::Rec601.to_string());
                    if rec601_button.clicked() {
                        config
                            .spectrum_calibration
                            .set_gain_preset(GainPresets::Rec601);
                    }
                    let rec709_button = ui.button(GainPresets::Rec709.to_string());
                    if rec709_button.clicked() {
                        config
                            .spectrum_calibration
                            .set_gain_preset(GainPresets::Rec709);
                    }
                });

                ui.separator();
                // Reference calibration settings. See readme for more information.
                let set_calibration_button = ui.add_enabled(
                    config.reference_config.reference.is_some()
                        && config.spectrum_calibration.scaling.is_none(),
                    Button::new("Set Reference as Calibration"),
                );
                if set_calibration_button.clicked() {
                    spectrum_container.set_calibration(
                        &mut config.spectrum_calibration,
                        &config.reference_config,
                    );
                };
                let delete_calibration_button = ui.add_enabled(
                    config.reference_config.reference.is_some()
                        && config.spectrum_calibration.scaling.is_some(),
                    Button::new("Delete Calibration"),
                );
                if delete_calibration_button.clicked() {
                    config.spectrum_calibration.scaling = None;
                };

                ui.separator();
                let set_zero_button = ui.add_enabled(
                    !spectrum_container.has_zero_reference(),
                    Button::new("Set Current As Zero Reference"),
                );
                if set_zero_button.clicked() {
                    spectrum_container.set_zero_reference();
                }
                let clear_zero_button = ui.add_enabled(
                    spectrum_container.has_zero_reference(),
                    Button::new("Clear Zero Reference"),
                );
                if clear_zero_button.clicked() {
                    spectrum_container.clear_zero_reference();
                }
            });
    }
}
