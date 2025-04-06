use crate::camera::{CameraEvent, CameraInfo, SharedFrameBuffer};
use crate::color::wavelength_to_color;
use crate::config::{GainPresets, Linearize, SpectrometerConfig, SpectrumPoint};
use crate::spectrum::{SpectrumContainer, SpectrumRgb};
use crate::{ThreadId, ThreadResult};
use eframe::{App, CreationContext};
use egui::{
    Button, Color32, ColorImage, ComboBox, Context, CornerRadius, Rect, RichText, Sense, Slider,
    Stroke, TextureHandle, UiBuilder, Vec2,
};
use egui_plot::{Legend, Line, MarkerShape, Plot, PlotPoint, Points, Polygon, Text, VLine};
use flume::{Receiver, Sender};
use image::EncodableLayout;
use indexmap::IndexMap;
use log::{debug, error, trace};
use nokhwa::pixel_format::RgbFormat;
use nokhwa::utils::{
    ApiBackend, CameraControl, CameraFormat, ControlValueDescription, ControlValueSetter,
    KnownCameraControlFlag,
};
use nokhwa::utils::{CameraIndex, RequestedFormat, RequestedFormatType};
use nokhwa::{Camera, query};
use std::borrow::BorrowMut;
use std::cmp::min;
use std::time::Duration;

mod import_export;

pub struct SpectrometerGui {
    config: SpectrometerConfig,
    import_export_window: import_export::ImportExportWindow,
    running: bool,
    camera_info: IndexMap<CameraIndex, crate::camera::CameraInfo>,
    camera_controls: Vec<CameraControl>,
    webcam_texture_id: Option<TextureHandle>,
    spectrum_container: SpectrumContainer,
    camera_config_tx: Sender<CameraEvent>,
    camera_config_change_pending: bool,
    result_rx: Receiver<ThreadResult>,
    frame_rx: SharedFrameBuffer,
    last_error: Option<ThreadResult>,
}

impl SpectrometerGui {
    pub fn new(
        cc: &CreationContext<'_>,
        camera_config_tx: Sender<CameraEvent>,
        spectrum_rx: Receiver<SpectrumRgb>,
        result_rx: Receiver<ThreadResult>,
        frame_rx: SharedFrameBuffer,
    ) -> Self {
        let config = if let Some(storage) = cc.storage {
            eframe::get_value(storage, eframe::APP_KEY).unwrap_or_default()
        } else {
            Default::default()
        };

        let mut gui = Self {
            config,
            import_export_window: import_export::ImportExportWindow::new(),
            running: false,
            camera_info: Default::default(),
            camera_controls: Default::default(),
            webcam_texture_id: None,
            spectrum_container: SpectrumContainer::new(spectrum_rx),
            camera_config_tx,
            camera_config_change_pending: false,
            result_rx,
            frame_rx,
            last_error: None,
        };
        gui.query_cameras();
        gui
    }

    fn query_cameras(&mut self) {
        for info in query(ApiBackend::Auto).unwrap_or_default().iter() {
            for format_type in crate::camera::CameraInfo::get_default_camera_format_types() {
                match Camera::new(
                    info.index().clone(),
                    RequestedFormat::new::<RgbFormat>(format_type),
                )
                .borrow_mut()
                {
                    Ok(cam) => {
                        let mut formats = cam.compatible_camera_formats().unwrap_or_default();
                        formats.sort_by_key(CameraFormat::width);
                        self.camera_info.insert(
                            info.index().clone(),
                            CameraInfo {
                                info: info.clone(),
                                formats,
                            },
                        );
                        break;
                    }
                    Err(e) => {
                        log::warn!("Could not open camera {info} with format {format_type}: {e}")
                    }
                }
            }
            if !self.camera_info.contains_key(info.index()) {
                log::warn!("Could not query camera {}", info);
            }
        }
    }

    fn send_config(&self) {
        self.camera_config_tx
            .send(CameraEvent::Config(self.config.image_config.clone()))
            .unwrap();
    }

    fn start_stream(&mut self) {
        self.refresh_controls();
        self.spectrum_container.clear_buffer();
        self.send_config();
        self.camera_config_tx
            .send(CameraEvent::StartStream {
                id: self
                    .camera_info
                    .get_index(self.config.camera_id)
                    .unwrap()
                    .0
                    .clone(),
                format: self.config.camera_format.unwrap(),
            })
            .unwrap();
    }

    fn refresh_controls(&mut self) {
        let requested_format = RequestedFormat::new::<RgbFormat>(RequestedFormatType::Exact(
            self.config.camera_format.unwrap(),
        ));
        match Camera::new(
            CameraIndex::Index(self.config.camera_id as u32),
            requested_format,
        ) {
            Ok(cam) => {
                self.camera_controls = cam
                    .camera_controls()
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|c| {
                        !c.flag().contains(&KnownCameraControlFlag::ReadOnly)
                            && !c.flag().contains(&KnownCameraControlFlag::WriteOnly)
                    })
                    .collect();
            }
            Err(e) => {
                error!("Could not refresh camera controls: {e}");
            }
        }
        if let Ok(cam) = Camera::new(
            CameraIndex::Index(self.config.camera_id as u32),
            requested_format,
        ) {
            self.camera_controls = cam
                .camera_controls()
                .unwrap_or_default()
                .into_iter()
                .filter(|c| {
                    !c.flag().contains(&KnownCameraControlFlag::ReadOnly)
                        && !c.flag().contains(&KnownCameraControlFlag::WriteOnly)
                })
                .collect();
        }
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
                        if self.config.view_config.draw_color_polygons {
                            for polygon in self.get_spectrum_color_polygons() {
                                plot_ui.polygon(polygon);
                            }
                        }
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
        let points = self
            .spectrum_container
            .get_spectrum_channel(index, &self.config)
            .into_iter()
            .map(|sp| [sp.wavelength as f64, sp.value as f64])
            .collect::<Vec<_>>();
        trace!(
            "Got {} points from spectrum for index {}: {:?}",
            points.len(),
            index,
            &points[..min(points.len(), 50)]
        );
        Line::new(points)
    }

    fn get_spectrum_color_polygons(&self) -> Vec<Polygon> {
        self.spectrum_container
            .get_spectrum_channel(3, &self.config)
            .as_slice()
            .windows(2)
            .map(|w| {
                Polygon::new(vec![
                    [w[0].wavelength as f64, 0.0],
                    [w[0].wavelength as f64, w[0].value as f64],
                    [w[1].wavelength as f64, w[1].value as f64],
                    [w[1].wavelength as f64, 0.0],
                ])
                .fill_color(wavelength_to_color(w[0].wavelength))
                .stroke(Stroke::new(0.0, Color32::TRANSPARENT))
            })
            .collect()
    }

    fn peaks_dips_to_plot(
        filtered_peaks_dips: &[SpectrumPoint],
        peaks: bool,
        max_spectrum_value: f32,
    ) -> (Points<'static>, Vec<Text>) {
        let mut peak_dip_labels = Vec::new();

        for peak_dip in filtered_peaks_dips {
            peak_dip_labels.push(
                Text::new(
                    PlotPoint::new(
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
            Points::new(
                filtered_peaks_dips
                    .iter()
                    .map(|sp| [sp.wavelength as f64, sp.value as f64])
                    .collect::<Vec<_>>(),
            )
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

                if let Some(webcam_texture_handle) = &self.webcam_texture_id {
                    let image = egui::Image::from_texture((
                        webcam_texture_handle.id(),
                        webcam_texture_handle.size_vec2(),
                    ))
                    .fit_to_exact_size(
                        webcam_texture_handle.size_vec2() * self.config.view_config.image_scale,
                    );
                    let image_response = ui.add(image);

                    // Paint window rect
                    ui.scope_builder(UiBuilder::new().layer_id(image_response.layer_id), |ui| {
                        let painter = ui.painter();
                        let image_rect = image_response.rect;
                        let image_origin = image_rect.min;
                        let scale = Vec2::new(
                            image_rect.width() / self.config.camera_format.unwrap().width() as f32,
                            image_rect.height()
                                / self.config.camera_format.unwrap().height() as f32,
                        );
                        let window_rect = Rect::from_min_size(
                            image_origin + self.config.image_config.window.offset * scale,
                            self.config.image_config.window.size * scale,
                        );
                        painter.rect_stroke(
                            window_rect,
                            CornerRadius::ZERO,
                            Stroke::new(2., Color32::GOLD),
                            egui::StrokeKind::Middle,
                        );
                    });
                    ui.separator();
                }

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
                // Reference calibration settings. See readme for more information.
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
                ui.separator();
                ui.checkbox(
                    &mut self.config.view_config.draw_color_polygons,
                    "Show colors under spectrum",
                );
            });
    }

    fn draw_camera_control_window(&mut self, ctx: &Context) {
        if self.config.view_config.show_camera_control_window {
            self.refresh_controls();
        }
        egui::Window::new("Camera Controls")
            .open(&mut self.config.view_config.show_camera_control_window)
            .show(ctx, |ui| {
                ui.colored_label(
                    Color32::YELLOW,
                    "⚠ Opening this window can increase load. ⚠",
                );
                let mut changed_controls = vec![];
                for ctrl in &mut self.camera_controls {
                    let mut value_setter = None;
                    match ctrl.value() {
                        ControlValueSetter::Integer(mut value) => {
                            if let ControlValueDescription::IntegerRange {
                                min,
                                max,
                                value: _,
                                step,
                                default,
                            } = ctrl.description()
                            {
                                ui.horizontal(|ui| {
                                    if ui.button("Reset").clicked() {
                                        value_setter = Some(ControlValueSetter::Integer(*default));
                                    }
                                    if ui
                                        .add(
                                            Slider::new(&mut value, (*min + 1)..=(*max - 1))
                                                .step_by(*step as f64)
                                                .text(ctrl.name()),
                                        )
                                        .changed()
                                    {
                                        value_setter = Some(ControlValueSetter::Integer(value));
                                    }
                                });
                            }
                        }
                        ControlValueSetter::Boolean(mut value) => {
                            if let ControlValueDescription::Boolean { default, .. } =
                                ctrl.description()
                            {
                                ui.horizontal(|ui| {
                                    if ui.button("Reset").clicked() {
                                        value_setter = Some(ControlValueSetter::Boolean(*default));
                                    }
                                    if ui.checkbox(&mut value, ctrl.name()).changed() {
                                        value_setter = Some(ControlValueSetter::Boolean(value))
                                    }
                                });
                            }
                        }
                        control => {
                            debug!("Control that cannot be represented: {control:?}");
                        }
                    };
                    if let Some(value_setter) = value_setter {
                        changed_controls.push((ctrl.control(), value_setter));
                        self.spectrum_container.clear_buffer();
                    };
                }
                if !changed_controls.is_empty() {
                    // Cannot use self.send_config due to mutable borrow in open
                    self.camera_config_tx
                        .send(CameraEvent::Controls(changed_controls))
                        .unwrap();
                }
            });
    }

    fn draw_windows(&mut self, ctx: &Context) {
        self.draw_camera_window(ctx);
        self.draw_calibration_window(ctx);
        self.draw_postprocessing_window(ctx);
        self.draw_camera_control_window(ctx);
        if let Some(last_error) =
            self.import_export_window
                .update(ctx, &mut self.config, &mut self.spectrum_container)
        {
            self.last_error = Some(last_error);
        }
    }

    fn draw_connection_panel(&mut self, ctx: &Context) {
        egui::TopBottomPanel::top("camera").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ComboBox::from_id_salt("cb_camera")
                    .selected_text(format!(
                        "{}: {}",
                        self.config.camera_id,
                        self.camera_info
                            .get_index(self.config.camera_id)
                            .map(|(_index, info)| info.info.human_name())
                            .unwrap_or_default()
                    ))
                    .show_ui(ui, |ui| {
                        if !self.running {
                            for (i, (_camera_index, camera_info)) in
                                self.camera_info.iter().enumerate()
                            {
                                ui.selectable_value(
                                    &mut self.config.camera_id,
                                    i,
                                    format!("{}: {}", i, camera_info.info.human_name()),
                                );
                            }
                        }
                    });
                ComboBox::from_id_salt("cb_camera_format")
                    .selected_text(match self.config.camera_format {
                        None => "".to_string(),
                        Some(camera_format) => format!("{}", camera_format),
                    })
                    .show_ui(ui, |ui| {
                        if !self.running {
                            if let Some((_camera_index, camera_info)) =
                                self.camera_info.get_index(self.config.camera_id)
                            {
                                for cf in &camera_info.formats {
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
            if let Some(camera_format) = self.config.camera_format {
                ctx.request_repaint_after(Duration::from_millis(
                    (1000 / camera_format.frame_rate()).into(),
                ));
            } else {
                ctx.request_repaint();
            }
        }

        match self.frame_rx.lock() {
            Ok(mut webcam_image) => {
                if let Some(webcam_image) = webcam_image.take() {
                    let webcam_color_image = ColorImage::from_rgb(
                        [
                            webcam_image.width() as usize,
                            webcam_image.height() as usize,
                        ],
                        webcam_image.as_bytes(),
                    );
                    self.webcam_texture_id = Some(ctx.load_texture(
                        "webcam_texture",
                        webcam_color_image,
                        Default::default(),
                    ));
                }
            }
            _ => {
                error!("Webcam thread poisoned lock");
            }
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
}

impl App for SpectrometerGui {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.update(ctx);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, &self.config);
    }
}
