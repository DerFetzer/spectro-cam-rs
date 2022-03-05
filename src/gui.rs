use crate::config::SpectrometerConfig;
use crate::spectrum::Spectrum;
use crate::CameraEvent;
use biquad::{
    Biquad, Coefficients, DirectForm2Transposed, Hertz, ToHertz, Type, Q_BUTTERWORTH_F32,
};
use egui::plot::{Legend, Line, Plot, Value, Values};
use egui::{Color32, Context, Rect, Rounding, Stroke, TextureId, Vec2};
use flume::{Receiver, Sender};
use rayon::prelude::*;

pub struct SpectrometerGui {
    config: SpectrometerConfig,
    webcam_texture_id: TextureId,
    camera_active: bool,
    spectrum: Vec<f32>,
    spectrum_buffer: Vec<Spectrum>,
    camera_config_tx: Sender<CameraEvent>,
    spectrum_rx: Receiver<Spectrum>,
}

impl SpectrometerGui {
    pub fn new(
        webcam_texture_id: TextureId,
        camera_config_tx: Sender<CameraEvent>,
        spectrum_rx: Receiver<Spectrum>,
    ) -> Self {
        let config = confy::load("spectro-cam-rs", None).unwrap_or_default();
        Self {
            config,
            webcam_texture_id,
            spectrum: Vec::new(),
            spectrum_buffer: Vec::new(),
            camera_active: false,
            camera_config_tx,
            spectrum_rx,
        }
    }

    fn start_stream(&mut self) {
        self.spectrum_buffer.clear();
        self.camera_config_tx
            .send(CameraEvent::Config(self.config.image_config.clone()))
            .unwrap();
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
        self.spectrum_buffer.insert(0, spectrum);
        self.spectrum_buffer
            .truncate(self.config.spectrum_buffer_size);
        self.spectrum = (self
            .spectrum_buffer
            .par_iter()
            .cloned()
            .reduce(|| Spectrum::from_element(ncols, 0.), |a, b| a + b)
            / self.config.spectrum_buffer_size as f32)
            .data
            .into();

        if let Some(cutoff) = self.config.spectrum_filter_cutoff {
            let cutoff = cutoff.clamp(0.001, 1.);
            let fs: Hertz<f32> = 2.0.hz();
            let f0: Hertz<f32> = cutoff.hz();

            let coeffs =
                Coefficients::<f32>::from_params(Type::LowPass, fs, f0, Q_BUTTERWORTH_F32).unwrap();
            let mut biquad = DirectForm2Transposed::<f32>::new(coeffs);
            for wl in self.spectrum.iter_mut() {
                *wl = biquad.run(*wl);
            }
        }
    }

    fn draw_spectrum(&mut self, ctx: &Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            let spectrum = Line::new({
                let calibration = self.config.spectrum_calibration;
                Values::from_values(
                    self.spectrum
                        .par_iter()
                        .enumerate()
                        .map(|(i, p)| {
                            let x = calibration.get_wavelength_from_index(i);
                            let y = *p;
                            Value::new(x, y)
                        })
                        .collect(),
                )
            });
            Plot::new("Spectrum")
                .legend(Legend::default())
                .show(ui, |plot_ui| {
                    plot_ui.line(spectrum);
                });
        });
    }

    fn draw_camera_window(&mut self, ctx: &Context) {
        egui::Window::new("Camera").show(ctx, |ui| {
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
        });
    }

    pub fn update(&mut self, ctx: &Context) {
        if self.camera_active {
            ctx.request_repaint();
        }

        if let Ok(spectrum) = self.spectrum_rx.try_recv() {
            self.update_spectrum(spectrum);
        }

        egui::TopBottomPanel::top("camera").show(ctx, |ui| {
            let connect_button = ui.button(if self.camera_active {
                "Stop..."
            } else {
                "Start..."
            });
            if connect_button.clicked() {
                self.camera_active = !self.camera_active;
                if self.camera_active {
                    self.start_stream();
                } else {
                    self.stop_stream();
                };
            }
        });
        self.draw_camera_window(ctx);
        self.draw_spectrum(ctx);
    }
}
