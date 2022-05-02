use glium::glutin;
use glium::texture::RawImage2d;
use glium::texture::SrgbTexture2d;
use nokhwa::{CameraFormat, FrameFormat, Resolution, ThreadedCamera};
use std::rc::Rc;

fn create_display(event_loop: &glutin::event_loop::EventLoop<()>) -> glium::Display {
    let window_builder = glutin::window::WindowBuilder::new()
        .with_resizable(true)
        .with_inner_size(glutin::dpi::LogicalSize {
            width: 800.0,
            height: 600.0,
        })
        .with_title("egui_glium example");

    let context_builder = glutin::ContextBuilder::new()
        .with_depth_buffer(0)
        .with_srgb(true)
        .with_stencil_buffer(0)
        .with_vsync(true);

    glium::Display::new(window_builder, context_builder, event_loop).unwrap()
}

fn main() {
    let event_loop = glutin::event_loop::EventLoop::with_user_event();
    let display = create_display(&event_loop);

    let mut egui_glium = egui_glium::EguiGlium::new(&display);

    // Load to gpu memory
    let glium_texture = SrgbTexture2d::empty(&display, 1280, 720).unwrap();
    // Allow us to share the texture with egui:
    let glium_texture = std::rc::Rc::new(glium_texture);
    // Allocate egui's texture id for GL texture
    let texture_id = egui_glium.painter.register_native_texture(glium_texture);
    // Setup button image size for reasonable image size for button container.
    let button_image_size = egui::Vec2::new(32_f32, 32_f32);

    let (tx, rx) = flume::unbounded();

    std::thread::spawn(move || {
        let mut camera = ThreadedCamera::new(0, None).unwrap();

        camera
            .set_camera_format(CameraFormat::new(
                Resolution::new(1280, 720),
                FrameFormat::MJPEG,
                10,
            ))
            .unwrap();
        camera.open_stream(|_| {}).unwrap();

        loop {
            let frame = camera.poll_frame().unwrap();
            tx.send(frame).unwrap();
        }
    });

    let mut dimensions = None;

    event_loop.run(move |event, _, control_flow| {
        if let Ok(frame) = rx.try_recv() {
            let dim = frame.dimensions();
            let image = RawImage2d::from_raw_rgb(frame.into_raw(), dim);
            let tex = SrgbTexture2d::new(&display, image).unwrap();
            egui_glium
                .painter
                .replace_native_texture(texture_id, Rc::new(tex));
            dimensions = Some(dim)
        };

        let mut redraw = || {
            let mut quit = false;

            let needs_repaint = egui_glium.run(&display, |egui_ctx| {
                egui::SidePanel::left("my_side_panel").show(egui_ctx, |ui| {
                    egui_ctx.request_repaint();
                    if ui
                        .add(egui::Button::image_and_text(
                            texture_id,
                            button_image_size,
                            "Quit",
                        ))
                        .clicked()
                    {
                        quit = true;
                    }
                });
                if let Some(dimensions) = dimensions {
                    egui::Window::new("NativeTextureDisplay").show(egui_ctx, |ui| {
                        ui.image(
                            texture_id,
                            egui::Vec2::new(dimensions.0 as f32, dimensions.1 as f32),
                        );
                    });
                };
            });

            *control_flow = if quit {
                glutin::event_loop::ControlFlow::Exit
            } else if needs_repaint {
                display.gl_window().window().request_redraw();
                glutin::event_loop::ControlFlow::Poll
            } else {
                glutin::event_loop::ControlFlow::Wait
            };

            {
                use glium::Surface as _;
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
                    *control_flow = glutin::event_loop::ControlFlow::Exit;
                }

                egui_glium.on_event(&event);

                display.gl_window().window().request_redraw(); // TODO: ask egui if the events warrants a repaint instead
            }

            _ => (),
        }
    });
}
