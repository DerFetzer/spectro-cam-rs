use crate::tungsten_halogen::reference_from_filament_temp;
use crate::{ThreadId, ThreadResult};
use egui::{Button, Context, Slider};

pub struct ImportExportWindow {
    tungsten_filament_temp: u16,
}

impl ImportExportWindow {
    pub fn new() -> Self {
        Self {
            tungsten_filament_temp: 2800,
        }
    }

    pub fn update(
        &mut self,
        ctx: &Context,
        config: &mut crate::config::SpectrometerConfig,
        spectrum_container: &mut crate::spectrum::SpectrumContainer,
    ) -> Option<ThreadResult> {
        let mut last_error = None;
        egui::Window::new("Import/Export")
            .open(&mut config.view_config.show_import_export_window)
            .show(ctx, |ui| {
                ui.text_edit_singleline(&mut config.import_export_config.path);
                ui.separator();
                let import_reference_button = ui.button("Import Reference CSV");
                if import_reference_button.clicked() {
                    match csv::Reader::from_path(&config.import_export_config.path)
                        .and_then(|mut r| r.deserialize().collect())
                    {
                        Ok(r) => {
                            config.reference_config.reference = Some(r);
                            last_error = Some(ThreadResult {
                                id: ThreadId::Main,
                                result: Ok(()),
                            });
                        }
                        Err(e) => {
                            last_error = Some(ThreadResult {
                                id: ThreadId::Main,
                                result: Err(e.to_string()),
                            });
                        }
                    };
                }
                let export_reference_button = ui.add_enabled(
                    config.reference_config.reference.is_some(),
                    Button::new("Export Reference CSV"),
                );
                if export_reference_button.clicked() {
                    let writer = csv::Writer::from_path(&config.import_export_config.path);
                    match writer {
                        Ok(mut writer) => {
                            for p in config.reference_config.reference.as_ref().unwrap() {
                                writer.serialize(p).unwrap();
                            }
                            writer.flush().unwrap();
                        }
                        Err(e) => {
                            last_error = Some(ThreadResult {
                                id: ThreadId::Main,
                                result: Err(e.to_string()),
                            })
                        }
                    }
                }
                let delete_button = ui.add_enabled(
                    config.reference_config.reference.is_some(),
                    Button::new("Delete Reference"),
                );
                if delete_button.clicked() {
                    config.reference_config.reference = None;
                }
                ui.separator();
                let generate_reference_button =
                    ui.button("Generate Reference From Tungsten Temperature");
                if generate_reference_button.clicked() {
                    config.reference_config.reference =
                        Some(reference_from_filament_temp(self.tungsten_filament_temp));
                }
                ui.add(
                    Slider::new(&mut self.tungsten_filament_temp, 1000..=3500)
                        .text("Tungsten Temperature"),
                );
                ui.separator();
                let export_button = ui.add(Button::new("Export Spectrum"));
                if export_button.clicked() {
                    match spectrum_container.write_to_csv(
                        &config.import_export_config.path.clone(),
                        &config.spectrum_calibration,
                    ) {
                        Ok(()) => {
                            last_error = Some(ThreadResult {
                                id: ThreadId::Main,
                                result: Ok(()),
                            });
                        }
                        Err(e) => {
                            last_error = Some(ThreadResult {
                                id: ThreadId::Main,
                                result: Err(e),
                            });
                        }
                    }
                }
            });
        last_error
    }
}
