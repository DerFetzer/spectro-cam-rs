use crate::camera::CameraInfo;
use crate::config::{CameraControl, GainPresets, Linearize, SpectrometerConfig, SpectrumPoint};
use crate::spectrum::{SpectrumContainer, SpectrumRgb};
use crate::tungsten_halogen::reference_from_filament_temp;
use crate::CameraEvent;
use egui::plot::{Legend, Line, MarkerShape, Plot, Points, Text, VLine, Value, Values};
use egui::{
    Button, Color32, ComboBox, Context, Rect, RichText, Rounding, Sense, Slider, Stroke, TextureId,
    Vec2,
};
use flume::{Receiver, Sender};
use glium::glutin::dpi::PhysicalSize;
use nokhwa::{query, Camera};
use spectro_cam_rs::{ThreadId, ThreadResult};
use std::any::Any;
use std::borrow::BorrowMut;
use std::collections::HashMap;

#[cfg(target_os = "linux")]
use v4l::{
    control::{Description, Flags},
    Control,
};

pub struct SpectrometerGui {
    config: SpectrometerConfig,
    running: bool,
    camera_info: HashMap<usize, CameraInfo>,
    camera_raw_controls: Vec<Box<dyn Any>>,
    camera_controls: Vec<CameraControl>,
    webcam_texture_id: TextureId,
    spectrum_container: SpectrumContainer,
    tungsten_filament_temp: u16,
    camera_config_tx: Sender<CameraEvent>,
    camera_config_change_pending: bool,
    result_rx: Receiver<ThreadResult>,
    last_error: Option<ThreadResult>,
}

impl SpectrometerGui {
    pub fn new(
        webcam_texture_id: TextureId,
        camera_config_tx: Sender<CameraEvent>,
        spectrum_rx: Receiver<SpectrumRgb>,
        config: SpectrometerConfig,
        result_rx: Receiver<ThreadResult>,
    ) -> Self {
        let mut gui = Self {
            config,
            running: false,
            camera_info: Default::default(),
            camera_raw_controls: Default::default(),
            camera_controls: Default::default(),
            webcam_texture_id,
            spectrum_container: SpectrumContainer::new(spectrum_rx),
            tungsten_filament_temp: 2800,
            camera_config_tx,
            camera_config_change_pending: false,
            result_rx,
            last_error: None,
        };
        gui.query_cameras();
        gui
    }

    fn query_cameras(&mut self) {
        let default_camera_formats = CameraInfo::get_default_camera_formats();

        for i in query()
            .unwrap_or_default()
            .iter()
            .map(nokhwa::CameraInfo::index)
        {
            for format in &default_camera_formats {
                if let Ok(cam) = Camera::new(i, Some(*format)).borrow_mut() {
                    let mut formats = cam.compatible_camera_formats().unwrap_or_default();
                    formats.sort_by_key(nokhwa::CameraFormat::width);
                    self.camera_info.insert(
                        i,
                        CameraInfo {
                            info: cam.info().clone(),
                            formats,
                        },
                    );
                    break;
                }
            }
            if !self.camera_info.contains_key(&i) {
                log::warn!("Could not query camera {}", i);
            }
        }
    }

    fn send_config(&self) {
        self.camera_config_tx
            .send(CameraEvent::Config(self.config.image_config.clone()))
            .unwrap();
    }

    fn start_stream(&mut self) {
        let default_camera_formats = CameraInfo::get_default_camera_formats();
        for format in default_camera_formats {
            if let Ok(cam) = Camera::new(self.config.camera_id, Some(format)) {
                let raw_controls = Self::get_raw_controls(&cam);

                self.camera_controls = Self::get_controls_from_raw_controls(&cam, &raw_controls);
                self.camera_raw_controls = raw_controls;
                break;
            }
        }
        self.spectrum_container.clear_buffer();
        self.send_config();
        self.camera_config_tx
            .send(CameraEvent::StartStream {
                id: self.config.camera_id,
                format: self.config.camera_format.unwrap(),
            })
            .unwrap();
    }

    #[cfg(target_os = "linux")]
    fn get_raw_controls(cam: &Camera) -> Vec<Box<dyn Any>> {
        cam.raw_supported_camera_controls()
            .unwrap_or_default()
            .into_iter()
            .filter(|c| match c.downcast_ref::<Description>() {
                None => false,
                Some(c) => {
                    !c.flags.contains(Flags::READ_ONLY) && !c.flags.contains(Flags::WRITE_ONLY)
                }
            })
            .collect()
    }

    #[cfg(target_os = "linux")]
    fn get_controls_from_raw_controls(
        cam: &Camera,
        raw_controls: &[Box<dyn Any>],
    ) -> Vec<CameraControl> {
        raw_controls
            .iter()
            .filter_map(|ctrl| {
                let descr = match ctrl.downcast_ref::<Description>() {
                    None => return None,
                    Some(descr) => descr,
                };
                if descr.flags.contains(Flags::READ_ONLY) || descr.flags.contains(Flags::WRITE_ONLY)
                {
                    None
                } else {
                    let rcc = *cam
                        .raw_camera_control(&descr.id)
                        .map(|c| c.downcast::<Control>().unwrap())
                        .unwrap();
                    let value = match rcc {
                        Control::Value(v) => v,
                        _ => return None,
                    };
                    Some(CameraControl {
                        id: descr.id,
                        name: descr.name.clone(),
                        value,
                    })
                }
            })
            .collect()
    }

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    fn get_raw_controls(_cam: &Camera) -> Vec<Box<dyn Any>> {
        Vec::new()
    }

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    fn get_controls_from_raw_controls(
        _cam: Camera,
        _raw_controls: &Vec<Box<dyn Any>>,
    ) -> Vec<CameraControl> {
        Vec::new()
    }

    fn stop_stream(&mut self) {
        self.camera_config_tx.send(CameraEvent::StopStream).unwrap();
    }

    fn draw_spectrum(&mut self, ctx: &Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            Plot::new("Spectrum")
                .legend(Legend::default())
                .show(ui, |plot_ui| {
                    if self.config.view_config.draw_spectrum_r {
                        plot_ui.line(self.get_spectrum_line(0).color(Color32::RED).name("r"));
                    }
                    if self.config.view_config.draw_spectrum_g {
                        plot_ui.line(self.get_spectrum_line(1).color(Color32::GREEN).name("g"));
                    }
                    if self.config.view_config.draw_spectrum_b {
                        plot_ui.line(self.get_spectrum_line(2).color(Color32::BLUE).name("b"));
                    }
                    if self.config.view_config.draw_spectrum_combined {
                        plot_ui.line(
                            self.get_spectrum_line(3)
                                .color(Color32::LIGHT_GRAY)
                                .name("sum"),
                        );
                    }

                    if self.config.view_config.draw_peaks || self.config.view_config.draw_dips {
                        let max_spectrum_value = self
                            .spectrum_container
                            .get_spectrum_max_value()
                            .unwrap_or_default();

                        if self.config.view_config.draw_peaks {
                            let filtered_peaks = self
                                .spectrum_container
                                .spectrum_to_peaks_and_dips(true, &self.config);

                            let (peaks, peak_labels) =
                                Self::peaks_dips_to_plot(&filtered_peaks, true, max_spectrum_value);

                            plot_ui.points(peaks);
                            for peak_label in peak_labels {
                                plot_ui.text(peak_label);
                            }
                        }
                        if self.config.view_config.draw_dips {
                            let filtered_dips = self
                                .spectrum_container
                                .spectrum_to_peaks_and_dips(false, &self.config);

                            let (dips, dip_labels) =
                                Self::peaks_dips_to_plot(&filtered_dips, false, max_spectrum_value);

                            plot_ui.points(dips);
                            for dip_label in dip_labels {
                                plot_ui.text(dip_label);
                            }
                        }
                    }

                    let line = self.config.reference_config.to_line();

                    if let Some(reference) = line {
                        plot_ui.line(reference.color(Color32::KHAKI).name("reference"));
                    }

                    if self.config.view_config.show_calibration_window {
                        plot_ui.vline(VLine::new(self.config.spectrum_calibration.low.wavelength));
                        plot_ui.vline(VLine::new(self.config.spectrum_calibration.high.wavelength));
                    }
                });
        });
    }

    fn get_spectrum_line(&self, index: usize) -> Line {
        Line::new({
            Values::from_values_iter(
                self.spectrum_container
                    .get_spectrum_channel(index, &self.config)
                    .into_iter()
                    .map(|sp| Value {
                        x: sp.wavelength as f64,
                        y: sp.value as f64,
                    }),
            )
        })
    }

    fn peaks_dips_to_plot(
        filtered_peaks_dips: &Vec<SpectrumPoint>,
        peaks: bool,
        max_spectrum_value: f32,
    ) -> (Points, Vec<Text>) {
        let mut peak_dip_labels = Vec::new();

        for peak_dip in filtered_peaks_dips {
            peak_dip_labels.push(
                Text::new(
                    Value::new(
                        peak_dip.wavelength,
                        if peaks {
                            peak_dip.value + (max_spectrum_value * 0.01)
                        } else {
                            peak_dip.value - (max_spectrum_value * 0.01)
                        },
                    ),
                    format!("{}", peak_dip.wavelength as u32),
                )
                .color(if peaks {
                    Color32::LIGHT_RED
                } else {
                    Color32::LIGHT_BLUE
                }),
            );
        }

        let (peaks, peak_labels) = (
            Points::new(Values::from_values_iter(
                filtered_peaks_dips
                    .iter()
                    .map(|sp| Value::new(sp.wavelength, sp.value)),
            ))
            .name("Peaks")
            .shape(if peaks {
                MarkerShape::Up
            } else {
                MarkerShape::Down
            })
            .color(if peaks {
                Color32::LIGHT_RED
            } else {
                Color32::LIGHT_BLUE
            })
            .filled(true)
            .radius(5.),
            peak_dip_labels,
        );
        (peaks, peak_labels)
    }

    fn draw_camera_window(&mut self, ctx: &Context) {
        egui::Window::new("Camera")
            .open(&mut self.config.view_config.show_camera_window)
            .show(ctx, |ui| {
                ui.add(
                    Slider::new(&mut self.config.view_config.image_scale, 0.1..=2.)
                        .text("Preview Scaling Factor"),
                );

                ui.separator();

                let image_size = egui::Vec2::new(
                    self.config.camera_format.unwrap().width() as f32,
                    self.config.camera_format.unwrap().height() as f32,
                ) * self.config.view_config.image_scale;
                let image_response = ui.image(self.webcam_texture_id, image_size);

                // Paint window rect
                ui.with_layer_id(image_response.layer_id, |ui| {
                    let painter = ui.painter();
                    let image_rect = image_response.rect;
                    let image_origin = image_rect.min;
                    let scale = Vec2::new(
                        image_rect.width() / self.config.camera_format.unwrap().width() as f32,
                        image_rect.height() / self.config.camera_format.unwrap().height() as f32,
                    );
                    let window_rect = Rect::from_min_size(
                        image_origin + self.config.image_config.window.offset * scale,
                        self.config.image_config.window.size * scale,
                    );
                    painter.rect_stroke(
                        window_rect,
                        Rounding::none(),
                        Stroke::new(2., Color32::GOLD),
                    );
                });
                ui.separator();

                // Window config
                let mut changed = false;

                ui.columns(2, |cols| {
                    changed |= cols[0]
                        .add(
                            Slider::new(
                                &mut self.config.image_config.window.offset.x,
                                1.0..=(self.config.camera_format.unwrap().width() as f32 - 1.),
                            )
                            .step_by(1.)
                            .text("Offset X"),
                        )
                        .changed();
                    changed |= cols[0]
                        .add(
                            Slider::new(
                                &mut self.config.image_config.window.offset.y,
                                1.0..=(self.config.camera_format.unwrap().height() as f32 - 1.),
                            )
                            .step_by(1.)
                            .text("Offset Y"),
                        )
                        .changed();

                    changed |= cols[1]
                        .add(
                            Slider::new(
                                &mut self.config.image_config.window.size.x,
                                1.0..=(self.config.camera_format.unwrap().width() as f32
                                    - self.config.image_config.window.offset.x
                                    - 1.),
                            )
                            .step_by(1.)
                            .text("Size X"),
                        )
                        .changed();
                    changed |= cols[1]
                        .add(
                            Slider::new(
                                &mut self.config.image_config.window.size.y,
                                1.0..=(self.config.camera_format.unwrap().height() as f32
                                    - self.config.image_config.window.offset.y
                                    - 1.),
                            )
                            .step_by(1.)
                            .text("Size Y"),
                        )
                        .changed();
                });
                ui.separator();
                changed |= ui
                    .checkbox(&mut self.config.image_config.flip, "Flip")
                    .changed();

                if changed {
                    self.camera_config_change_pending = true;
                }

                ui.separator();
                let update_config_button = ui.add(Button::new("Update Config").sense(
                    if self.camera_config_change_pending {
                        Sense::click()
                    } else {
                        Sense::hover()
                    },
                ));
                if update_config_button.clicked() {
                    self.camera_config_change_pending = false;
                    // Cannot use self.send_config due to mutable borrow in open
                    self.camera_config_tx
                        .send(CameraEvent::Config(self.config.image_config.clone()))
                        .unwrap();
                }
            });
    }

    fn draw_calibration_window(&mut self, ctx: &Context) {
        egui::Window::new("Calibration")
            .open(&mut self.config.view_config.show_calibration_window)
            .show(ctx, |ui| {
                ui.add(
                    Slider::new(
                        &mut self.config.spectrum_calibration.low.wavelength,
                        200..=self.config.spectrum_calibration.high.wavelength - 1,
                    )
                    .text("Low Wavelength"),
                );
                ui.add(
                    Slider::new(
                        &mut self.config.spectrum_calibration.low.index,
                        0..=self.config.spectrum_calibration.high.index - 1,
                    )
                    .text("Low Index"),
                );

                ui.add(
                    Slider::new(
                        &mut self.config.spectrum_calibration.high.wavelength,
                        (self.config.spectrum_calibration.low.wavelength + 1)..=2000,
                    )
                    .text("High Wavelength"),
                );
                ui.add(
                    Slider::new(
                        &mut self.config.spectrum_calibration.high.index,
                        (self.config.spectrum_calibration.low.index + 1)
                            ..=self.config.image_config.window.size.x as usize,
                    )
                    .text("High Index"),
                );
                ui.separator();
                ComboBox::from_label("Linearize")
                    .selected_text(self.config.spectrum_calibration.linearize.to_string())
                    .show_ui(ui, |ui| {
                        let mut changed = false;
                        changed |= ui
                            .selectable_value(
                                &mut self.config.spectrum_calibration.linearize,
                                Linearize::Off,
                                Linearize::Off.to_string(),
                            )
                            .changed();
                        changed |= ui
                            .selectable_value(
                                &mut self.config.spectrum_calibration.linearize,
                                Linearize::Rec601,
                                Linearize::Rec601.to_string(),
                            )
                            .changed();
                        changed |= ui
                            .selectable_value(
                                &mut self.config.spectrum_calibration.linearize,
                                Linearize::Rec709,
                                Linearize::Rec709.to_string(),
                            )
                            .changed();
                        changed |= ui
                            .selectable_value(
                                &mut self.config.spectrum_calibration.linearize,
                                Linearize::SRgb,
                                Linearize::SRgb.to_string(),
                            )
                            .changed();

                        // Clear buffer if value changed
                        if changed {
                            self.spectrum_container.clear_buffer()
                        };
                    });
                ui.add(
                    Slider::new(&mut self.config.spectrum_calibration.gain_r, 0.0..=10.)
                        .text("Gain R"),
                );
                ui.add(
                    Slider::new(&mut self.config.spectrum_calibration.gain_g, 0.0..=10.)
                        .text("Gain G"),
                );
                ui.add(
                    Slider::new(&mut self.config.spectrum_calibration.gain_b, 0.0..=10.)
                        .text("Gain B"),
                );

                ui.horizontal(|ui| {
                    let unity_button = ui.button(GainPresets::Unity.to_string());
                    if unity_button.clicked() {
                        self.config
                            .spectrum_calibration
                            .set_gain_preset(GainPresets::Unity);
                    }
                    let srgb_button = ui.button(GainPresets::SRgb.to_string());
                    if srgb_button.clicked() {
                        self.config
                            .spectrum_calibration
                            .set_gain_preset(GainPresets::SRgb);
                    }
                    let rec601_button = ui.button(GainPresets::Rec601.to_string());
                    if rec601_button.clicked() {
                        self.config
                            .spectrum_calibration
                            .set_gain_preset(GainPresets::Rec601);
                    }
                    let rec709_button = ui.button(GainPresets::Rec709.to_string());
                    if rec709_button.clicked() {
                        self.config
                            .spectrum_calibration
                            .set_gain_preset(GainPresets::Rec709);
                    }
                });

                ui.separator();
                let set_calibration_button = ui.add_enabled(
                    self.config.reference_config.reference.is_some()
                        && self.config.spectrum_calibration.scaling.is_none(),
                    Button::new("Set Reference as Calibration"),
                );
                if set_calibration_button.clicked() {
                    self.spectrum_container.set_calibration(
                        &mut self.config.spectrum_calibration,
                        &self.config.reference_config,
                    );
                };
                let delete_calibration_button = ui.add_enabled(
                    self.config.reference_config.reference.is_some()
                        && self.config.spectrum_calibration.scaling.is_some(),
                    Button::new("Delete Calibration"),
                );
                if delete_calibration_button.clicked() {
                    self.config.spectrum_calibration.scaling = None;
                };

                ui.separator();
                let set_zero_button = ui.add_enabled(
                    !self.spectrum_container.has_zero_reference(),
                    Button::new("Set Current As Zero Reference"),
                );
                if set_zero_button.clicked() {
                    self.spectrum_container.set_zero_reference();
                }
                let clear_zero_button = ui.add_enabled(
                    self.spectrum_container.has_zero_reference(),
                    Button::new("Clear Zero Reference"),
                );
                if clear_zero_button.clicked() {
                    self.spectrum_container.clear_zero_reference();
                }
            });
    }

    fn draw_postprocessing_window(&mut self, ctx: &Context) {
        egui::Window::new("Postprocessing")
            .open(&mut self.config.view_config.show_postprocessing_window)
            .show(ctx, |ui| {
                ui.add(
                    Slider::new(
                        &mut self.config.postprocessing_config.spectrum_buffer_size,
                        1..=100,
                    )
                    .text("Averaging Buffer Size"),
                );
                ui.separator();
                ui.horizontal(|ui| {
                    ui.checkbox(
                        &mut self.config.postprocessing_config.spectrum_filter_active,
                        "Low-Pass Filter",
                    );
                    ui.add_enabled(
                        self.config.postprocessing_config.spectrum_filter_active,
                        Slider::new(
                            &mut self.config.postprocessing_config.spectrum_filter_cutoff,
                            0.001..=1.,
                        )
                        .logarithmic(true)
                        .text("Cutoff"),
                    );
                });
                ui.separator();
                ui.add_enabled(
                    self.config.reference_config.reference.is_some(),
                    Slider::new(&mut self.config.reference_config.scale, 0.001..=100.)
                        .logarithmic(true)
                        .text("Reference Scale"),
                );
                ui.separator();
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.config.view_config.draw_peaks, "Show Peaks");
                    ui.checkbox(&mut self.config.view_config.draw_dips, "Show Dips");
                });
                ui.add(
                    Slider::new(&mut self.config.view_config.peaks_dips_find_window, 1..=200)
                        .text("Peaks/Dips Find Window"),
                );
                ui.add(
                    Slider::new(
                        &mut self.config.view_config.peaks_dips_unique_window,
                        1.0..=200.,
                    )
                    .text("Peaks/Dips Filter Window"),
                );
            });
    }

    #[cfg(target_os = "linux")]
    fn draw_camera_control_window(&mut self, ctx: &Context) {
        egui::Window::new("Camera Controls")
            .open(&mut self.config.view_config.show_camera_control_window)
            .show(ctx, |ui| {
                let mut changed_controls = vec![];
                for ctrl in &mut self.camera_raw_controls {
                    let ctrl = match ctrl.downcast_ref::<Description>() {
                        None => continue,
                        Some(ctrl) => ctrl,
                    };
                    let own_ctrl = match self.camera_controls.iter_mut().find(|c| c.id == ctrl.id) {
                        None => continue,
                        Some(own_ctrl) => own_ctrl,
                    };
                    let value_changed = match ctrl.typ {
                        v4l::control::Type::Integer => ui
                            .add(
                                Slider::new(
                                    &mut own_ctrl.value,
                                    (ctrl.minimum + 1)..=(ctrl.maximum - 1),
                                )
                                .step_by(ctrl.step as f64)
                                .text(&ctrl.name),
                            )
                            .changed(),
                        v4l::control::Type::Boolean => {
                            let mut checked = own_ctrl.value == 1;
                            let response = ui.checkbox(&mut checked, &ctrl.name);
                            own_ctrl.value = checked as i32;
                            response.changed()
                        }
                        v4l::control::Type::Menu => {
                            let mut changed = false;
                            let items = match ctrl.items.as_ref() {
                                None => continue,
                                Some(items) => items,
                            };
                            let selected_text =
                                match items.iter().find(|&i| i.0 == own_ctrl.value as u32) {
                                    None => continue,
                                    Some(i) => i.1.to_string(),
                                };
                            ComboBox::from_label(&ctrl.name)
                                .selected_text(selected_text)
                                .show_ui(ui, |ui| {
                                    for item in items.iter() {
                                        changed |= ui
                                            .selectable_value(
                                                &mut own_ctrl.value,
                                                item.0 as i32,
                                                item.1.to_string(),
                                            )
                                            .changed();
                                    }
                                });
                            changed
                        }
                        _ => false,
                    };
                    if value_changed {
                        changed_controls.push(own_ctrl.clone());
                        self.spectrum_container.clear_buffer();
                    };
                }
                let default_button = ui.button("All default");
                if default_button.clicked() {
                    for ctrl in &mut self.camera_raw_controls {
                        let ctrl = match ctrl.downcast_ref::<Description>() {
                            None => continue,
                            Some(ctrl) => ctrl,
                        };
                        let own_ctrl =
                            match self.camera_controls.iter_mut().find(|c| c.id == ctrl.id) {
                                None => continue,
                                Some(own_ctrl) => own_ctrl,
                            };

                        own_ctrl.value = ctrl.default;
                    }
                    // Cannot use self.send_config due to mutable borrow in open
                    self.camera_config_tx
                        .send(CameraEvent::Controls(self.camera_controls.clone()))
                        .unwrap();
                }
                if !changed_controls.is_empty() {
                    // Cannot use self.send_config due to mutable borrow in open
                    self.camera_config_tx
                        .send(CameraEvent::Controls(changed_controls))
                        .unwrap();
                }
            });
    }

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    fn draw_camera_control_window(&mut self, _ctx: &Context) {}

    fn draw_import_export_window(&mut self, ctx: &Context) {
        egui::Window::new("Import/Export")
            .open(&mut self.config.view_config.show_import_export_window)
            .show(ctx, |ui| {
                ui.text_edit_singleline(&mut self.config.import_export_config.path);
                ui.separator();
                let load_button = ui.button("Import Reference CSV");
                if load_button.clicked() {
                    match csv::Reader::from_path(&self.config.import_export_config.path)
                        .and_then(|mut r| r.deserialize().collect())
                    {
                        Ok(r) => {
                            self.config.reference_config.reference = Some(r);
                            self.last_error = Some(ThreadResult {
                                id: ThreadId::Main,
                                result: Ok(()),
                            });
                        }
                        Err(e) => {
                            self.last_error = Some(ThreadResult {
                                id: ThreadId::Main,
                                result: Err(e.to_string()),
                            });
                        }
                    };
                }
                let delete_button = ui.add_enabled(
                    self.config.reference_config.reference.is_some(),
                    Button::new("Delete Reference"),
                );
                if delete_button.clicked() {
                    self.config.reference_config.reference = None;
                }
                ui.separator();
                let generate_reference_button =
                    ui.button("Generate Reference From Tungsten Temperature");
                if generate_reference_button.clicked() {
                    self.config.reference_config.reference =
                        Some(reference_from_filament_temp(self.tungsten_filament_temp));
                }
                ui.add(
                    Slider::new(&mut self.tungsten_filament_temp, 1000..=3500)
                        .text("Tungsten Temperature"),
                );
                ui.separator();
                let export_button = ui.add(Button::new("Export Spectrum"));
                if export_button.clicked() {
                    match self.spectrum_container.write_to_csv(
                        &self.config.import_export_config.path.clone(),
                        &self.config.spectrum_calibration,
                    ) {
                        Ok(()) => {
                            self.last_error = Some(ThreadResult {
                                id: ThreadId::Main,
                                result: Ok(()),
                            });
                        }
                        Err(e) => {
                            self.last_error = Some(ThreadResult {
                                id: ThreadId::Main,
                                result: Err(e),
                            });
                        }
                    }
                }
            });
    }

    fn draw_windows(&mut self, ctx: &Context) {
        self.draw_camera_window(ctx);
        self.draw_calibration_window(ctx);
        self.draw_postprocessing_window(ctx);
        self.draw_camera_control_window(ctx);
        self.draw_import_export_window(ctx);
    }

    fn draw_connection_panel(&mut self, ctx: &Context) {
        egui::TopBottomPanel::top("camera").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ComboBox::from_id_source("cb_camera")
                    .selected_text(format!(
                        "{}: {}",
                        self.config.camera_id,
                        self.camera_info
                            .get(&self.config.camera_id)
                            .map(|ci| ci.info.human_name())
                            .unwrap_or_default()
                    ))
                    .show_ui(ui, |ui| {
                        if !self.running {
                            for (i, ci) in &self.camera_info {
                                ui.selectable_value(
                                    &mut self.config.camera_id,
                                    *i,
                                    format!("{}: {}", i, ci.info.human_name()),
                                );
                            }
                        }
                    });
                ComboBox::from_id_source("cb_camera_format")
                    .selected_text(match self.config.camera_format {
                        None => "".to_string(),
                        Some(camera_format) => format!("{}", camera_format),
                    })
                    .show_ui(ui, |ui| {
                        if !self.running {
                            if let Some(ci) = self.camera_info.get(&self.config.camera_id) {
                                for cf in &ci.formats {
                                    ui.selectable_value(
                                        &mut self.config.camera_format,
                                        Some(*cf),
                                        format!("{}", cf),
                                    );
                                }
                            }
                        }
                    });

                let connect_button = ui.button(if self.running { "Stop..." } else { "Start..." });
                if connect_button.clicked() {
                    if self.config.camera_format.is_some() {
                        // Clamp window values to camera-resolution
                        let camera_format = self.config.camera_format.unwrap();
                        self.config
                            .image_config
                            .clamp(camera_format.width() as f32, camera_format.height() as f32);

                        self.running = !self.running;
                        if self.running {
                            self.start_stream();
                        } else {
                            self.stop_stream();
                        };
                    } else {
                        self.last_error = Some(ThreadResult {
                            id: ThreadId::Main,
                            result: Err("Choose a camera format!".to_string()),
                        });
                    }
                };
            });
        });
    }

    fn draw_window_selection_panel(&mut self, ctx: &Context) {
        egui::SidePanel::left("window_selection").show(ctx, |ui| {
            ui.checkbox(&mut self.config.view_config.show_camera_window, "Camera");
            ui.checkbox(
                &mut self.config.view_config.show_camera_control_window,
                "Camera Controls",
            );
            ui.checkbox(
                &mut self.config.view_config.show_calibration_window,
                "Calibration",
            );
            ui.checkbox(
                &mut self.config.view_config.show_postprocessing_window,
                "Postprocessing",
            );
            ui.checkbox(
                &mut self.config.view_config.show_import_export_window,
                "Import/Export",
            );
        });
    }

    fn draw_last_result(&mut self, ctx: &Context) {
        egui::TopBottomPanel::bottom("result").show(ctx, |ui| {
            if let Some(res) = self.last_error.as_ref() {
                ui.label(match &res.result {
                    Ok(()) => RichText::new("OK").color(Color32::GREEN),
                    Err(e) => RichText::new(format!("Error: {}", e)).color(Color32::RED),
                })
            } else {
                ui.label("")
            }
        });
    }

    fn handle_thread_result(&mut self, res: &ThreadResult) {
        if let ThreadResult {
            id: ThreadId::Camera,
            result: Err(_),
        } = res
        {
            self.running = false;
        }
    }

    pub fn update(&mut self, ctx: &Context) {
        if self.running {
            ctx.request_repaint();
        }

        self.spectrum_container.update(&self.config);

        if let Ok(error) = self.result_rx.try_recv() {
            self.handle_thread_result(&error);
            self.last_error = Some(error);
        }

        self.draw_connection_panel(ctx);

        if self.running {
            self.draw_window_selection_panel(ctx);
            self.draw_windows(ctx);
        }

        self.draw_spectrum(ctx);
        self.draw_last_result(ctx);
    }

    pub fn persist_config(&mut self, window_size: PhysicalSize<u32>) {
        self.config.view_config.window_size = window_size;
        if let Err(e) = confy::store("spectro-cam-rs", None, self.config.clone()) {
            log::error!("Could not persist config: {:?}", e);
        }
    }
}
