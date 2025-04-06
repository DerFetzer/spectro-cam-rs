use crate::camera::SharedFrameBuffer;
use egui::{
    Button, Color32, ColorImage, Context, CornerRadius, Rect, Sense, Slider, Stroke, TextureHandle,
    UiBuilder, Vec2,
};
use image::EncodableLayout;
use log::error;

pub struct CameraWindow {
    frame_rx: SharedFrameBuffer,
    webcam_texture_id: Option<TextureHandle>,
    camera_config_change_pending: bool,
}

impl CameraWindow {
    pub fn new(frame_rx: SharedFrameBuffer) -> Self {
        Self {
            frame_rx,
            webcam_texture_id: None,
            camera_config_change_pending: false,
        }
    }

    /// Updates and draws the camera window. Returns true if the spectrum window settings changed.
    pub fn update(
        &mut self,
        ctx: &Context,
        config: &mut crate::config::SpectrometerConfig,
    ) -> bool {
        self.check_for_new_frame(ctx);

        let mut window_config_changed = false;
        egui::Window::new("Camera")
            .open(&mut config.view_config.show_camera_window)
            .show(ctx, |ui| {
                ui.add(
                    Slider::new(&mut config.view_config.image_scale, 0.1..=2.)
                        .text("Preview Scaling Factor"),
                );

                ui.separator();

                if let Some(webcam_texture_handle) = &self.webcam_texture_id {
                    let image = egui::Image::from_texture((
                        webcam_texture_handle.id(),
                        webcam_texture_handle.size_vec2(),
                    ))
                    .fit_to_exact_size(
                        webcam_texture_handle.size_vec2() * config.view_config.image_scale,
                    );
                    let image_response = ui.add(image);

                    // Paint window rect
                    ui.scope_builder(UiBuilder::new().layer_id(image_response.layer_id), |ui| {
                        let painter = ui.painter();
                        let image_rect = image_response.rect;
                        let image_origin = image_rect.min;
                        let scale = Vec2::new(
                            image_rect.width() / config.camera_format.unwrap().width() as f32,
                            image_rect.height() / config.camera_format.unwrap().height() as f32,
                        );
                        let window_rect = Rect::from_min_size(
                            image_origin + config.image_config.window.offset * scale,
                            config.image_config.window.size * scale,
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
                                &mut config.image_config.window.offset.x,
                                1.0..=(config.camera_format.unwrap().width() as f32 - 1.),
                            )
                            .step_by(1.)
                            .text("Offset X"),
                        )
                        .changed();
                    changed |= cols[0]
                        .add(
                            Slider::new(
                                &mut config.image_config.window.offset.y,
                                1.0..=(config.camera_format.unwrap().height() as f32 - 1.),
                            )
                            .step_by(1.)
                            .text("Offset Y"),
                        )
                        .changed();

                    changed |= cols[1]
                        .add(
                            Slider::new(
                                &mut config.image_config.window.size.x,
                                1.0..=(config.camera_format.unwrap().width() as f32
                                    - config.image_config.window.offset.x
                                    - 1.),
                            )
                            .step_by(1.)
                            .text("Size X"),
                        )
                        .changed();
                    changed |= cols[1]
                        .add(
                            Slider::new(
                                &mut config.image_config.window.size.y,
                                1.0..=(config.camera_format.unwrap().height() as f32
                                    - config.image_config.window.offset.y
                                    - 1.),
                            )
                            .step_by(1.)
                            .text("Size Y"),
                        )
                        .changed();
                });
                ui.separator();
                changed |= ui.checkbox(&mut config.image_config.flip, "Flip").changed();

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
                    window_config_changed = true;
                }
            });
        window_config_changed
    }

    /// Checks if a new camera frame is available and updates the texture if so.
    fn check_for_new_frame(&mut self, ctx: &Context) {
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
    }
}
