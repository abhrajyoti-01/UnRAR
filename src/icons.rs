//! Embedded toolbar/list icons (generated pixel-art PNGs in assets/).

use egui::{ColorImage, Context, TextureHandle, TextureOptions};
use std::collections::HashMap;

macro_rules! icon {
    ($map:expr, $ctx:expr, $name:literal) => {
        let img = egui_extras::image::load_image_bytes(
            include_bytes!(concat!("../assets/", $name, ".png")))
            .expect(concat!("icon ", $name));
        $map.insert($name, $ctx.load_texture($name, img, TextureOptions::default()));
    };
}

pub struct Icons {
    pub map: HashMap<&'static str, TextureHandle>,
}

impl Icons {
    pub fn load(ctx: &Context) -> Self {
        let mut map = HashMap::new();
        icon!(map, ctx, "add");
        icon!(map, ctx, "comment");
        icon!(map, ctx, "delete");
        icon!(map, ctx, "extract");
        icon!(map, ctx, "file");
        icon!(map, ctx, "find");
        icon!(map, ctx, "folder");
        icon!(map, ctx, "help");
        icon!(map, ctx, "info");
        icon!(map, ctx, "lock");
        icon!(map, ctx, "new");
        icon!(map, ctx, "open");
        icon!(map, ctx, "test");
        icon!(map, ctx, "up");
        icon!(map, ctx, "view");
        Icons { map }
    }

    pub fn get(&self, name: &str) -> TextureHandle {
        self.map.get(name).expect("icon").clone()
    }
}

/// App icon (32x32 RGBA) for the window title bar.
pub fn app_icon() -> egui::IconData {
    let img: ColorImage = egui_extras::image::load_image_bytes(
        include_bytes!("../assets/app.png")).expect("app icon");
    let size = [img.width() as u32, img.height() as u32];
    egui::IconData {
        rgba: img.as_raw().to_vec(),
        width: size[0],
        height: size[1],
    }
}
