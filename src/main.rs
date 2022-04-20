use egui::TextureId;
use egui_glium::EguiGlium;
use epi::NativeTexture;
use glium::texture::RawImage2d;
use glium::texture::SrgbTexture2d;
use glium::Surface as _;
use glium::{glutin, Display};
use spectro_cam_rs::camera::CameraThread;
use spectro_cam_rs::config::SpectrometerConfig;
use spectro_cam_rs::gui::SpectrometerGui;
use spectro_cam_rs::init_logging;
use spectro_cam_rs::spectrum::SpectrumCalculator;
use std::rc::Rc;

fn create_display(
    event_loop: &glutin::event_loop::EventLoop<()>,
    window_size: glutin::dpi::PhysicalSize<u32>,
) -> glium::Display {
    let window_builder = glutin::window::WindowBuilder::new()
        .with_resizable(true)
        .with_inner_size(window_size)
        .with_title("spectro-cam-rs");

    let context_builder = glutin::ContextBuilder::new()
        .with_depth_buffer(0)
        .with_srgb(true)
        .with_stencil_buffer(0)
        .with_vsync(true);

    let display = glium::Display::new(window_builder, context_builder, event_loop).unwrap();

    // Clear window
    let mut target = display.draw();

    let color = egui::Rgba::from_rgb(0., 0., 0.);
    target.clear_color(color[0], color[1], color[2], color[3]);

    target.finish().unwrap();

    display
}

fn register_webcam_texture(display: &Display, egui_glium: &mut EguiGlium) -> TextureId {
    let glium_texture = SrgbTexture2d::empty(display, 1, 1).unwrap();
    let glium_texture = std::rc::Rc::new(glium_texture);
    egui_glium.painter.register_native_texture(glium_texture)
}

fn load_config() -> SpectrometerConfig {
    confy::load("spectro-cam-rs", None).unwrap_or_default()
}

fn main() {
    init_logging();

    let config = load_config();

    let event_loop = glutin::event_loop::EventLoop::with_user_event();
    let display = create_display(&event_loop, config.view_config.window_size);

    let mut egui_glium = egui_glium::EguiGlium::new(&display);

    let texture_id = register_webcam_texture(&display, &mut egui_glium);

    let (frame_tx, frame_rx) = flume::unbounded();
    let (window_tx, window_rx) = flume::unbounded();
    let (spectrum_tx, spectrum_rx) = flume::unbounded();
    let (config_tx, config_rx) = flume::unbounded();
    let (result_tx, result_rx) = flume::unbounded();

    std::thread::spawn(move || CameraThread::new(frame_tx, window_tx, config_rx, result_tx).run());
    std::thread::spawn(move || SpectrumCalculator::new(window_rx, spectrum_tx).run());

    let mut gui = SpectrometerGui::new(texture_id, config_tx, spectrum_rx, config, result_rx);

    event_loop.run(move |event, _, control_flow| {
        if let Ok(frame) = frame_rx.try_recv() {
            let dim = frame.dimensions();
            let image = RawImage2d::from_raw_rgb(frame.into_raw(), dim);
            let tex = SrgbTexture2d::new(&display, image).unwrap();
            egui_glium
                .painter
                .replace_native_texture(texture_id, Rc::new(tex));
        };

        let mut redraw = || {
            let needs_repaint = egui_glium.run(&display, |egui_ctx| {
                gui.update(egui_ctx);
            });

            *control_flow = if needs_repaint {
                display.gl_window().window().request_redraw();
                glutin::event_loop::ControlFlow::Poll
            } else {
                glutin::event_loop::ControlFlow::Wait
            };

            {
                let mut target = display.draw();

                let color = egui::Rgba::from_rgb(0.1, 0.3, 0.2);
                target.clear_color(color[0], color[1], color[2], color[3]);

                // draw things behind egui here

                egui_glium.paint(&display, &mut target);

                // draw things on top of egui here

                target.finish().unwrap();
            }
        };

        match event {
            // Platform-dependent event handlers to workaround a winit bug
            // See: https://github.com/rust-windowing/winit/issues/987
            // See: https://github.com/rust-windowing/winit/issues/1619
            glutin::event::Event::RedrawEventsCleared if cfg!(windows) => redraw(),
            glutin::event::Event::RedrawRequested(_) if !cfg!(windows) => redraw(),

            glutin::event::Event::WindowEvent { event, .. } => {
                use glutin::event::WindowEvent;
                if matches!(event, WindowEvent::CloseRequested | WindowEvent::Destroyed) {
                    gui.persist_config(display.gl_window().window().inner_size());
                    *control_flow = glutin::event_loop::ControlFlow::Exit;
                }

                egui_glium.on_event(&event);

                display.gl_window().window().request_redraw(); // TODO: ask egui if the events warrants a repaint instead
            }

            _ => (),
        }
    });
}
