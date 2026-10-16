use std::{io::Cursor, sync::OnceLock};

pub const NAME: &str = "lapiSwitch";
pub const SUBTITLE: &str = "TB520FU · PRC ↔ ROW";
const PNG: &[u8] = include_bytes!("../assets/branding/lapiswitch.png");

struct Pixels {
    rgba: Vec<u8>,
    width: u32,
    height: u32,
}

fn pixels() -> &'static Pixels {
    static PIXELS: OnceLock<Pixels> = OnceLock::new();
    PIXELS.get_or_init(|| {
        let mut decoder = png::Decoder::new(Cursor::new(PNG));
        decoder.set_transformations(png::Transformations::normalize_to_color8());
        let mut reader = decoder.read_info().expect("embedded lapiSwitch PNG header");
        let mut rgba = vec![0; reader.output_buffer_size().expect("icon buffer size")];
        let info = reader
            .next_frame(&mut rgba)
            .expect("embedded lapiSwitch PNG pixels");
        assert_eq!(info.color_type, png::ColorType::Rgba);
        rgba.truncate(info.buffer_size());
        Pixels {
            rgba,
            width: info.width,
            height: info.height,
        }
    })
}

pub fn image() -> iced::widget::image::Handle {
    static HANDLE: OnceLock<iced::widget::image::Handle> = OnceLock::new();
    HANDLE
        .get_or_init(|| {
            let pixels = pixels();
            iced::widget::image::Handle::from_rgba(pixels.width, pixels.height, pixels.rgba.clone())
        })
        .clone()
}

pub fn window_icon() -> iced::window::Icon {
    let pixels = pixels();
    iced::window::icon::from_rgba(pixels.rgba.clone(), pixels.width, pixels.height)
        .expect("embedded lapiSwitch window icon")
}
