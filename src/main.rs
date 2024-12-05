use egui::TextureId;
use egui::ViewportId;
use egui_glium::EguiGlium;
use flume::Receiver;
use glium::backend::glutin::SimpleWindowBuilder;
use glium::glutin::surface::WindowSurface;
use glium::texture::RawImage2d;
use glium::texture::SrgbTexture2d;
use glium::Display;
use glium::Surface as _;
use image::ImageBuffer;
use image::Rgb;
use spectro_cam_rs::camera::CameraThread;
use spectro_cam_rs::config::SpectrometerConfig;
use spectro_cam_rs::gui::SpectrometerGui;
use spectro_cam_rs::init_logging;
use spectro_cam_rs::spectrum::SpectrumCalculator;
use std::rc::Rc;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::StartCause;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::event_loop::EventLoop;
use winit::window::Window;

fn create_display(
    event_loop: &EventLoop<()>,
    window_size: PhysicalSize<u32>,
) -> (winit::window::Window, glium::Display<WindowSurface>) {
    let (window, display) = SimpleWindowBuilder::new()
        .set_window_builder(Window::default_attributes().with_resizable(true))
        .with_inner_size(window_size.width, window_size.height)
        .with_title("spectro-cam-rs")
        .build(event_loop);

    // Clear window
    let mut target = display.draw();

    let color = egui::Rgba::from_rgb(0., 0., 0.);
    target.clear_color(color[0], color[1], color[2], color[3]);

    target.finish().unwrap();

    (window, display)
}

fn register_webcam_texture(
    display: &Display<WindowSurface>,
    egui_glium: &mut EguiGlium,
) -> TextureId {
    let glium_texture = SrgbTexture2d::empty(display, 1, 1).unwrap();
    let glium_texture = std::rc::Rc::new(glium_texture);
    egui_glium
        .painter
        .register_native_texture(Rc::clone(&glium_texture), Default::default())
}

fn load_config() -> SpectrometerConfig {
    confy::load("spectro-cam-rs", None).unwrap_or_default()
}

fn main() {
    init_logging();

    let config = load_config();

    let event_loop = EventLoop::new().unwrap();
    let (window, display) = create_display(&event_loop, config.view_config.window_size);

    let mut egui_glium =
        egui_glium::EguiGlium::new(ViewportId::ROOT, &display, &window, &event_loop);

    let texture_id = register_webcam_texture(&display, &mut egui_glium);

    let (frame_tx, frame_rx) = flume::unbounded();
    let (window_tx, window_rx) = flume::unbounded();
    let (spectrum_tx, spectrum_rx) = flume::unbounded();
    let (config_tx, config_rx) = flume::unbounded();
    let (result_tx, result_rx) = flume::unbounded();

    std::thread::spawn(move || CameraThread::new(frame_tx, window_tx, config_rx, result_tx).run());
    std::thread::spawn(move || SpectrumCalculator::new(window_rx, spectrum_tx).run());

    let gui = SpectrometerGui::new(texture_id, config_tx, spectrum_rx, config, result_rx);

    let mut app = App {
        egui_glium,
        texture_id,
        window,
        display,
        frame_rx,
        gui,
    };

    event_loop.run_app(&mut app).unwrap();
}

struct App {
    egui_glium: egui_glium::EguiGlium,
    texture_id: TextureId,
    window: winit::window::Window,
    display: glium::Display<WindowSurface>,
    frame_rx: Receiver<ImageBuffer<Rgb<u8>, Vec<u8>>>,
    gui: SpectrometerGui,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop) {}

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: winit::event::WindowEvent,
    ) {
        if let Ok(frame) = self.frame_rx.try_recv() {
            let dim = frame.dimensions();
            let image = RawImage2d::from_raw_rgb(frame.into_raw(), dim);
            let tex = SrgbTexture2d::new(&self.display, image).unwrap();
            self.egui_glium.painter.replace_native_texture(
                self.texture_id,
                Rc::new(tex),
                Default::default(),
            );
        };

        let mut redraw = || {
            self.egui_glium.run(&self.window, |egui_ctx| {
                self.gui.update(egui_ctx);
            });

            {
                let mut target = self.display.draw();

                let color = egui::Rgba::from_rgb(0.1, 0.3, 0.2);
                target.clear_color(color[0], color[1], color[2], color[3]);

                // draw things behind egui here

                self.egui_glium.paint(&self.display, &mut target);

                // draw things on top of egui here

                target.finish().unwrap();
            }
        };

        match &event {
            WindowEvent::CloseRequested | WindowEvent::Destroyed => event_loop.exit(),
            WindowEvent::Resized(new_size) => {
                self.display.resize((*new_size).into());
            }
            WindowEvent::RedrawRequested => redraw(),
            _ => {}
        }

        let event_response = self.egui_glium.on_event(&self.window, &event);

        if event_response.repaint {
            self.window.request_redraw();
        }
    }

    fn new_events(&mut self, _event_loop: &ActiveEventLoop, cause: StartCause) {
        if let StartCause::ResumeTimeReached { .. } = cause {
            self.window.request_redraw();
        }
    }
}
