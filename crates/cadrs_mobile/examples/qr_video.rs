//! Writes a QR code of a text as a short raw video (`.y4m`), for a browser's fake camera
//! (`chromium --use-file-for-fake-video-capture=…`): testing the phone page's scanning.
//!
//! `cargo run -p cadrs_mobile --example qr_video -- <text> <out.y4m>`

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(text), Some(out)) = (args.next(), args.next()) else {
        eprintln!("usage: qr_video <text> <out.y4m>");
        std::process::exit(2);
    };
    let code = qrcode::QrCode::new(text.as_bytes()).expect("fits a QR code");
    let modules = code.width();
    let colors = code.to_colors();
    let (w, h) = (640usize, 480usize);
    let scale = (h - 40) / (modules + 8);
    let side = (modules + 8) * scale;
    let (ox, oy) = ((w - side) / 2, (h - side) / 2);
    let mut luma = vec![200u8; w * h];
    for y in 0..side {
        for x in 0..side {
            let (mx, my) = ((x / scale) as isize - 4, (y / scale) as isize - 4);
            let dark = mx >= 0
                && my >= 0
                && (mx as usize) < modules
                && (my as usize) < modules
                && colors[my as usize * modules + mx as usize] == qrcode::Color::Dark;
            luma[(oy + y) * w + ox + x] = if dark { 16 } else { 235 };
        }
    }
    let chroma = vec![128u8; (w / 2) * (h / 2)];
    let mut video = format!("YUV4MPEG2 W{w} H{h} F10:1 Ip A1:1 C420jpeg\n").into_bytes();
    for _ in 0..20 {
        video.extend_from_slice(b"FRAME\n");
        video.extend_from_slice(&luma);
        video.extend_from_slice(&chroma);
        video.extend_from_slice(&chroma);
    }
    std::fs::write(&out, video).expect("written");
}
