use crate::camera::CameraInfo;
use crate::config::{CameraControl, SpectrometerConfig};
use crate::spectrum::Spectrum;
use crate::CameraEvent;
use biquad::{
    Biquad, Coefficients, DirectForm2Transposed, Hertz, ToHertz, Type, Q_BUTTERWORTH_F32,
};
use egui::plot::{Legend, Line, Plot, VLine, Value, Values};
use egui::{
    Button, Checkbox, Color32, ComboBox, Context, Rect, Rounding, Sense, Slider, Stroke, TextureId,
    Vec2,
};
use flume::{Receiver, Sender};
use nokhwa::{query, Camera};
use rayon::prelude::*;
use std::any::Any;
use std::borrow::BorrowMut;
use std::collections::HashMap;
use v4l::control::{Description, Flags};

pub struct SpectrometerGui {
    config: SpectrometerConfig,
    running: bool,
    camera_info: HashMap<usize, CameraInfo>,
    camera_controls: Vec<Box<dyn Any>>,
    webcam_texture_id: TextureId,
    spectrum: Spectrum,
    spectrum_buffer: Vec<Spectrum>,
    camera_config_tx: Sender<CameraEvent>,
    camera_config_change_pending: bool,
    spectrum_rx: Receiver<Spectrum>,
}

impl SpectrometerGui {
    pub fn new(
        webcam_texture_id: TextureId,
        camera_config_tx: Sender<CameraEvent>,
        spectrum_rx: Receiver<Spectrum>,
    ) -> Self {
        let config: SpectrometerConfig = confy::load("spectro-cam-rs", None).unwrap_or_default();
        let spectrum_width = config.camera_format.width();
        let mut gui = Self {
            config,
            running: false,
            camera_info: Default::default(),
            camera_controls: Default::default(),
            webcam_texture_id,
            spectrum: Spectrum::zeros(spectrum_width as usize),
            spectrum_buffer: Vec::new(),
            camera_config_tx,
            camera_config_change_pending: false,
            spectrum_rx,
        };
        gui.query_cameras();
        gui
    }

    fn query_cameras(&mut self) {
        let default_camera_formats = CameraInfo::get_default_camera_formats();

        for i in query().unwrap().iter().map(|c| c.index()) {
            for format in default_camera_formats.iter() {
                if let Ok(cam) = Camera::new(i, Some(*format)).borrow_mut() {
                    let mut formats = cam.compatible_camera_formats().unwrap();
                    formats.sort_by_key(|cf| cf.width());
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
                self.camera_controls = cam
                    .raw_supported_camera_controls()
                    .unwrap()
                    .into_iter()
                    .filter(|c| {
                        let c = c.downcast_ref::<Description>().unwrap();
                        !c.flags.contains(Flags::READ_ONLY) && !c.flags.contains(Flags::WRITE_ONLY)
                    })
                    .collect();

                self.config.image_config.controls = self
                    .camera_controls
                    .iter()
                    .filter_map(|c| {
                        let d = c.downcast_ref::<Description>().unwrap();
                        if d.flags.contains(Flags::READ_ONLY) || d.flags.contains(Flags::WRITE_ONLY)
                        {
                            None
                        } else {
                            Some(CameraControl {
                                id: d.id,
                                name: d.name.clone(),
                                value: d.default,
                            })
                        }
                    })
                    .collect();
                break;
            }
        }
        log::info!(
            "{:?}",
            self.camera_controls
                .iter()
                .map(|c| {
                    let c = c.downcast_ref::<Description>().unwrap();
                    (c.id, c.name.clone(), c.flags, c.typ)
                })
                .collect::<Vec<(u32, String, Flags, v4l::control::Type)>>()
        );
        log::info!("{:?}", self.config.image_config.controls);
        self.spectrum_buffer.clear();
        self.send_config();
        self.camera_config_tx
            .send(CameraEvent::StartStream {
                id: self.config.camera_id,
                format: self.config.camera_format,
            })
            .unwrap();
    }

    fn stop_stream(&mut self) {
        self.camera_config_tx.send(CameraEvent::StopStream).unwrap();
    }

    fn update_spectrum(&mut self, spectrum: Spectrum) {
        let ncols = spectrum.ncols();

        // Clear buffer on dimension change
        if let Some(s) = self.spectrum_buffer.get(0) {
            if s.len() != spectrum.len() {
                self.spectrum_buffer.clear();
            }
        }
        self.spectrum_buffer.insert(0, spectrum);
        self.spectrum_buffer
            .truncate(self.config.postprocessing_config.spectrum_buffer_size);
        self.spectrum = self
            .spectrum_buffer
            .par_iter()
            .cloned()
            .reduce(|| Spectrum::from_element(ncols, 0.), |a, b| a + b)
            / self.config.postprocessing_config.spectrum_buffer_size as f32;

        if self.config.postprocessing_config.spectrum_filter_active {
            let cutoff = self
                .config
                .postprocessing_config
                .spectrum_filter_cutoff
                .clamp(0.001, 1.);
            let fs: Hertz<f32> = 2.0.hz();
            let f0: Hertz<f32> = cutoff.hz();

            let coeffs =
                Coefficients::<f32>::from_params(Type::LowPass, fs, f0, Q_BUTTERWORTH_F32).unwrap();
            for mut channel in self.spectrum.row_iter_mut() {
                let mut biquad = DirectForm2Transposed::<f32>::new(coeffs);
                for sample in channel.iter_mut() {
                    *sample = biquad.run(*sample);
                }
            }
        }
    }

    fn spectrum_channel_to_line(&mut self, channel_index: usize) -> Line {
        Line::new({
            let calibration = self.config.spectrum_calibration;
            Values::from_values(
                self.spectrum
                    .row(channel_index)
                    .iter()
                    .enumerate()
                    .map(|(i, p)| {
                        let x = calibration.get_wavelength_from_index(i);
                        let y = *p;
                        Value::new(x, y)
                    })
                    .collect(),
            )
        })
    }

    fn draw_spectrum(&mut self, ctx: &Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            Plot::new("Spectrum")
                .legend(Legend::default())
                .show(ui, |plot_ui| {
                    if self.config.view_config.draw_spectrum_r {
                        plot_ui.line(
                            self.spectrum_channel_to_line(0)
                                .color(Color32::RED)
                                .name("r"),
                        );
                    }
                    if self.config.view_config.draw_spectrum_g {
                        plot_ui.line(
                            self.spectrum_channel_to_line(1)
                                .color(Color32::GREEN)
                                .name("g"),
                        );
                    }
                    if self.config.view_config.draw_spectrum_b {
                        plot_ui.line(
                            self.spectrum_channel_to_line(2)
                                .color(Color32::BLUE)
                                .name("b"),
                        );
                    }
                    if self.config.view_config.draw_spectrum_combined {
                        plot_ui.line(
                            self.spectrum_channel_to_line(3)
                                .color(Color32::LIGHT_GRAY)
                                .name("sum"),
                        );
                    }

                    if self.config.view_config.show_calibration_window {
                        plot_ui.vline(VLine::new(self.config.spectrum_calibration.low.wavelength));
                        plot_ui.vline(VLine::new(self.config.spectrum_calibration.high.wavelength));
                    }
                });
        });
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
                    self.config.camera_format.width() as f32,
                    self.config.camera_format.height() as f32,
                ) * self.config.view_config.image_scale;
                let image_response = ui.image(self.webcam_texture_id, image_size);

                // Paint window rect
                ui.with_layer_id(image_response.layer_id, |ui| {
                    let painter = ui.painter();
                    let image_rect = image_response.rect;
                    let image_origin = image_rect.min;
                    let scale = Vec2::new(
                        image_rect.width() / self.config.camera_format.width() as f32,
                        image_rect.height() / self.config.camera_format.height() as f32,
                    );
                    let window_rect = Rect::from_min_size(
                        image_origin + self.config.image_config.window.offset * scale,
                        self.config.image_config.window.size * scale,
                    );
                    painter.rect_stroke(
                        window_rect,
                        Rounding::none(),
                        Stroke::new(2., Color32::GOLD),
                    )
                });
                ui.separator();

                // Window config
                let mut changed = false;

                ui.columns(2, |cols| {
                    changed |= cols[0]
                        .add(
                            Slider::new(
                                &mut self.config.image_config.window.offset.x,
                                1.0..=(self.config.camera_format.width() as f32 - 1.),
                            )
                            .step_by(1.)
                            .text("Offset X"),
                        )
                        .changed();
                    changed |= cols[0]
                        .add(
                            Slider::new(
                                &mut self.config.image_config.window.offset.y,
                                1.0..=(self.config.camera_format.height() as f32 - 1.),
                            )
                            .step_by(1.)
                            .text("Offset Y"),
                        )
                        .changed();

                    changed |= cols[1]
                        .add(
                            Slider::new(
                                &mut self.config.image_config.window.size.x,
                                0.0..=(self.config.camera_format.width() as f32
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
                                0.0..=(self.config.camera_format.height() as f32
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
                        .text("Cutoff"),
                    );
                });
            });
    }

    fn draw_camera_control_window(&mut self, ctx: &Context) {
        egui::Window::new("Postprocessing")
            .open(&mut self.config.view_config.show_camera_control_window)
            .show(ctx, |ui| {
                let mut changed_controls = vec![];
                for ctrl in &mut self.camera_controls {
                    let ctrl = ctrl.downcast_ref::<Description>().unwrap();
                    let own_ctrl = self
                        .config
                        .image_config
                        .controls
                        .iter_mut()
                        .find(|c| c.id == ctrl.id)
                        .unwrap();
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
                            let response = ui.add(Checkbox::new(&mut checked, &ctrl.name));
                            own_ctrl.value = checked as i32;
                            response.changed()
                        }
                        v4l::control::Type::Menu => {
                            let mut changed = false;
                            ComboBox::from_label(&ctrl.name)
                                .selected_text(
                                    ctrl.items
                                        .as_ref()
                                        .unwrap()
                                        .iter()
                                        .find(|&i| i.0 == own_ctrl.value as u32)
                                        .unwrap()
                                        .1
                                        .to_string(),
                                )
                                .show_ui(ui, |ui| {
                                    for item in ctrl.items.as_ref().unwrap().iter() {
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
                    };
                }
                let default_button = ui.button("All default");
                if default_button.clicked() {
                    for ctrl in &mut self.camera_controls {
                        let ctrl = ctrl.downcast_ref::<Description>().unwrap();
                        let own_ctrl = self
                            .config
                            .image_config
                            .controls
                            .iter_mut()
                            .find(|c| c.id == ctrl.id)
                            .unwrap();

                        own_ctrl.value = ctrl.default;
                    }
                    // Cannot use self.send_config due to mutable borrow in open
                    self.camera_config_tx
                        .send(CameraEvent::Config(self.config.image_config.clone()))
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

    fn draw_windows(&mut self, ctx: &Context) {
        self.draw_camera_window(ctx);
        self.draw_calibration_window(ctx);
        self.draw_postprocessing_window(ctx);
        self.draw_camera_control_window(ctx);
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
                            .unwrap()
                            .info
                            .human_name()
                    ))
                    .show_ui(ui, |ui| {
                        if !self.running {
                            for (i, ci) in self.camera_info.iter() {
                                ui.selectable_value(
                                    &mut self.config.camera_id,
                                    *i,
                                    format!("{}: {}", i, ci.info.human_name()),
                                );
                            }
                        }
                    });
                ComboBox::from_id_source("cb_camera_format")
                    .selected_text(format!("{}", self.config.camera_format))
                    .show_ui(ui, |ui| {
                        if !self.running {
                            for cf in self
                                .camera_info
                                .get(&self.config.camera_id)
                                .unwrap()
                                .formats
                                .iter()
                            {
                                ui.selectable_value(
                                    &mut self.config.camera_format,
                                    *cf,
                                    format!("{}", cf),
                                );
                            }
                        }
                    });

                let connect_button = ui.button(if self.running { "Stop..." } else { "Start..." });
                if connect_button.clicked() {
                    self.running = !self.running;
                    if self.running {
                        self.start_stream();
                    } else {
                        self.stop_stream();
                    };
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
        });
    }

    pub fn update(&mut self, ctx: &Context) {
        if self.running {
            ctx.request_repaint();
        }

        if let Ok(spectrum) = self.spectrum_rx.try_recv() {
            self.update_spectrum(spectrum);
        }

        self.draw_connection_panel(ctx);
        self.draw_window_selection_panel(ctx);
        self.draw_windows(ctx);
        self.draw_spectrum(ctx);
    }
}
