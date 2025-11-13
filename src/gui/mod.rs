use crate::camera::{CameraEvent, CameraInfo, SharedFrameBuffer};
use crate::color::wavelength_to_color;
use crate::config::{SpectrometerConfig, SpectrumPoint};
use crate::spectrum::{SpectrumContainer, SpectrumRgb};
use crate::{ThreadId, ThreadResult};
use eframe::{App, CreationContext};
use egui::{Color32, ComboBox, Context, RichText, Stroke};
use egui_plot::{Legend, Line, MarkerShape, Plot, PlotPoint, Points, Polygon, Text, VLine};
use flume::{Receiver, Sender};
use indexmap::IndexMap;
use log::{error, trace};
use nokhwa::pixel_format::RgbFormat;
use nokhwa::utils::{ApiBackend, CameraControl, CameraFormat, KnownCameraControlFlag};
use nokhwa::utils::{CameraIndex, RequestedFormat, RequestedFormatType};
use nokhwa::{Camera, query};
use std::borrow::BorrowMut;
use std::cmp::min;
use std::time::Duration;

mod calibration;
mod camera;
mod camera_control;
mod import_export;
mod postprocessing;

pub struct SpectrometerGui {
    config: SpectrometerConfig,
    camera_window: camera::CameraWindow,
    calibration_window: calibration::CalibrationWindow,
    postprocessing_window: postprocessing::PostProcessingWindow,
    camera_control_window: camera_control::CameraControlWindow,
    import_export_window: import_export::ImportExportWindow,
    running: bool,
    paused: bool,
    camera_info: IndexMap<CameraIndex, crate::camera::CameraInfo>,
    camera_controls: Vec<CameraControl>,
    spectrum_container: SpectrumContainer,
    camera_config_tx: Sender<CameraEvent>,
    result_rx: Receiver<ThreadResult>,
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
            camera_window: camera::CameraWindow::new(frame_rx),
            calibration_window: calibration::CalibrationWindow::new(),
            postprocessing_window: postprocessing::PostProcessingWindow::new(),
            camera_control_window: camera_control::CameraControlWindow::new(),
            import_export_window: import_export::ImportExportWindow::new(),
            running: false,
            paused: false,
            camera_info: Default::default(),
            camera_controls: Default::default(),
            spectrum_container: SpectrumContainer::new(spectrum_rx),
            camera_config_tx,
            result_rx,
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

    fn pause_stream(&mut self) {
        self.camera_config_tx.send(CameraEvent::Pause).unwrap();
    }

    fn resume_stream(&mut self) {
        self.camera_config_tx.send(CameraEvent::Resume).unwrap();
    }

    fn stop_stream(&mut self) {
        // First resume stream, otherwise the camera thread will not continue
        self.resume_stream();
        self.camera_config_tx.send(CameraEvent::StopStream).unwrap();
    }

    fn draw_spectrum(&mut self, ctx: &Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            Plot::new("Spectrum")
                .legend(Legend::default())
                .show(ui, |plot_ui| {
                    if self.config.view_config.draw_spectrum_r {
                        plot_ui.line(
                            self.get_spectrum_line(0)
                                .color(Color32::RED)
                                .name("Spectrum R"),
                        );
                    }
                    if self.config.view_config.draw_spectrum_g {
                        plot_ui.line(
                            self.get_spectrum_line(1)
                                .color(Color32::GREEN)
                                .name("Spectrum G"),
                        );
                    }
                    if self.config.view_config.draw_spectrum_b {
                        plot_ui.line(
                            self.get_spectrum_line(2)
                                .color(Color32::BLUE)
                                .name("Spectrum B"),
                        );
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
                        plot_ui.vline(VLine::new(
                            "Low calibration wavelength",
                            self.config.spectrum_calibration.low.wavelength,
                        ));
                        plot_ui.vline(VLine::new(
                            "High calibration wavelength",
                            self.config.spectrum_calibration.high.wavelength,
                        ));
                    }
                });
        });
    }

    fn get_spectrum_line(&self, index: usize) -> Line<'_> {
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
        Line::new(format!("Spectrum line {index}"), points)
    }

    fn get_spectrum_color_polygons(&self) -> Vec<Polygon<'_>> {
        self.spectrum_container
            .get_spectrum_channel(3, &self.config)
            .as_slice()
            .windows(2)
            .map(|w| {
                Polygon::new(
                    "Spectrum colors",
                    vec![
                        [w[0].wavelength as f64, 0.0],
                        [w[0].wavelength as f64, w[0].value as f64],
                        [w[1].wavelength as f64, w[1].value as f64],
                        [w[1].wavelength as f64, 0.0],
                    ],
                )
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
                    if peaks {
                        "Peaks wavelength"
                    } else {
                        "Dips wavelength"
                    },
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
                if peaks {
                    "Peaks markers"
                } else {
                    "Dips markers"
                },
                filtered_peaks_dips
                    .iter()
                    .map(|sp| [sp.wavelength as f64, sp.value as f64])
                    .collect::<Vec<_>>(),
            )
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
        let window_config_changed = self.camera_window.update(ctx, &mut self.config);
        if window_config_changed {
            self.camera_config_tx
                .send(CameraEvent::Config(self.config.image_config.clone()))
                .unwrap();
        }
    }

    fn draw_camera_control_window(&mut self, ctx: &Context) {
        if self.config.view_config.show_camera_control_window {
            self.refresh_controls();
        }
        let changed_controls = self.camera_control_window.update(
            ctx,
            &mut self.config.view_config.show_camera_control_window,
            &self.camera_controls,
        );
        if !changed_controls.is_empty() {
            self.camera_config_tx
                .send(CameraEvent::Controls(changed_controls))
                .unwrap();
            // New camera settings means changed input to the spectrum. Clear buffer
            self.spectrum_container.clear_buffer();
        }
    }

    fn draw_windows(&mut self, ctx: &Context) {
        self.draw_camera_window(ctx);

        self.calibration_window
            .update(ctx, &mut self.config, &mut self.spectrum_container);

        self.postprocessing_window.update(ctx, &mut self.config);

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
                        if !self.running
                            && let Some((_camera_index, camera_info)) =
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
                        self.paused = false;
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
                if self.running {
                    let pause_resume_button =
                        ui.button(if self.paused { "Resume" } else { "Pause" });
                    if pause_resume_button.clicked() {
                        if self.paused {
                            self.resume_stream();
                        } else {
                            self.pause_stream();
                        }
                        self.paused = !self.paused;
                    }
                }
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
