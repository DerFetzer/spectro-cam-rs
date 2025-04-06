use egui::{Color32, Context, Slider};
use log::debug;
use nokhwa::utils::{
    CameraControl, ControlValueDescription, ControlValueSetter, KnownCameraControl,
};

pub struct CameraControlWindow {}

impl CameraControlWindow {
    pub fn new() -> Self {
        Self {}
    }

    /// Renders the camera control window and returns a vector of changed controls.
    pub fn update(
        &mut self,
        ctx: &Context,
        show_window: &mut bool,
        camera_controls: &[CameraControl],
    ) -> Vec<(KnownCameraControl, ControlValueSetter)> {
        let mut changed_controls = vec![];
        egui::Window::new("Camera Controls")
            .open(show_window)
            .show(ctx, |ui| {
                ui.colored_label(
                    Color32::YELLOW,
                    "⚠ Opening this window can increase load. ⚠",
                );
                for ctrl in camera_controls {
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
                    };
                }
            });
        changed_controls
    }
}
