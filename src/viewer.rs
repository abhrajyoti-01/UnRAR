//! Internal viewer — WinRAR-style file preview window (text / hex).

pub struct Viewer {
    pub name: String,
    pub data: Vec<u8>,
    pub hex: bool,
    pub open: bool,
    cached: Option<String>,
    cached_hex: bool,
}

fn is_binary(data: &[u8]) -> bool {
    let n = data.len().min(8192);
    if n == 0 { return false; }
    if data[..n].contains(&0) { return true; }
    let printable = data[..n].iter()
        .filter(|&&b| b == b'\t' || b == b'\n' || b == b'\r'
            || (32..=126).contains(&b) || b >= 128)
        .count();
    printable * 4 < n * 3
}

fn hexdump(data: &[u8]) -> String {
    let mut out = String::new();
    for (row, chunk) in data.chunks(16).enumerate() {
        let off = row * 16;
        let mut hx = String::new();
        for (i, b) in chunk.iter().enumerate() {
            if i == 8 { hx.push(' '); }
            hx.push_str(&format!("{:02x} ", b));
        }
        let asc: String = chunk.iter()
            .map(|&b| if (32..127).contains(&b) { b as char } else { '.' })
            .collect();
        out.push_str(&format!("{:08x}  {:<49}  {}\n", off, hx, asc));
    }
    out
}

fn to_text(data: &[u8]) -> String {
    match std::str::from_utf8(data) {
        Ok(s) => s.to_string(),
        Err(_) => data.iter()
            .map(|&b| if b >= 32 || b == b'\n' || b == b'\t'
                { b as char } else { ' ' })
            .collect(),
    }
}

impl Viewer {
    pub fn new(name: &str, data: Vec<u8>) -> Self {
        let hex = is_binary(&data);
        Viewer {
            name: name.to_string(), data, hex, open: true,
            cached: None, cached_hex: hex,
        }
    }

    fn body(&mut self) -> &str {
        if self.cached.is_none() || self.cached_hex != self.hex {
            self.cached = Some(if self.hex {
                hexdump(&self.data)
            } else {
                to_text(&self.data)
            });
            self.cached_hex = self.hex;
        }
        self.cached.as_deref().unwrap_or_default()
    }

    pub fn ui(&mut self, ctx: &egui::Context) {
        let mut open = self.open;
        let mut body_holder = String::new();
        egui::Window::new(format!("UnRAR viewer - {}", self.name))
            .open(&mut open)
            .default_size([760.0, 520.0])
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui.selectable_label(!self.hex, "Text").clicked() {
                        self.hex = false;
                    }
                    if ui.selectable_label(self.hex, "Hex").clicked() {
                        self.hex = true;
                    }
                    ui.with_layout(egui::Layout::right_to_left(
                        egui::Align::Center), |ui| {
                        let (nlines, mode) = if self.hex {
                            (self.data.len().div_ceil(16), "hex")
                        } else {
                            (to_text(&self.data).lines().count(), "text")
                        };
                        ui.label(format!("{} bytes | {} lines | {}",
                                         self.data.len(), nlines, mode));
                    });
                });
                ui.separator();
                body_holder = self.body().to_string();
                egui::ScrollArea::both()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.add(egui::TextEdit::multiline(&mut body_holder)
                            .font(egui::TextStyle::Monospace)
                            .desired_width(f32::INFINITY)
                            .interactive(true));
                    });
            });
        self.open = open;
    }
}
