//! Headless screenshots: rasterise a `Scene` with AcmeUI's CPU renderer and write a PNG.
//! Used by `--smoke-test` (`NOTEDIT_SCREENSHOT=<file.png>`) so the UI can be inspected
//! without a visible desktop.
use acme_core::Scene;

/// Render `scene` (which must carry every atlas upload it uses) into an RGB PNG.
pub fn write_png(scene: &Scene, width: f32, height: f32, scale: f32, path: &std::path::Path) -> Result<(), String> {
    let (w, h) = ((width * scale).round().max(1.0) as u32, (height * scale).round().max(1.0) as u32);
    let mut r = acme_render_cpu::CpuRenderer::new(w, h, scale).map_err(|e| e.to_string())?;
    r.render_scene(scene).map_err(|e| e.to_string())?;
    let px = r.pixels();
    let mut raw = Vec::with_capacity((w as usize * 3 + 1) * h as usize);
    for y in 0..h as usize {
        raw.push(0u8);
        for &p in &px[y * w as usize..(y + 1) * w as usize] { raw.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8]); }
    }
    std::fs::write(path, png(w, h, &raw)).map_err(|e| e.to_string())
}

fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xffff_ffffu32;
    for &b in data {
        c ^= b as u32;
        for _ in 0..8 { c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 }; }
    }
    !c
}

/// Minimal PNG: one IDAT with stored (uncompressed) deflate blocks.
fn png(w: u32, h: u32, raw: &[u8]) -> Vec<u8> {
    let mut z = vec![0x78, 0x01];
    let mut chunks = raw.chunks(65535).peekable();
    if chunks.peek().is_none() { z.extend_from_slice(&[1, 0, 0, 0xff, 0xff]); }
    while let Some(c) = chunks.next() {
        z.push(if chunks.peek().is_none() { 1 } else { 0 });
        let n = c.len() as u16;
        z.extend_from_slice(&n.to_le_bytes());
        z.extend_from_slice(&(!n).to_le_bytes());
        z.extend_from_slice(c);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in raw { a = (a + x as u32) % 65521; b = (b + a) % 65521; }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    let mut chunk = |kind: &[u8], data: &[u8]| {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend_from_slice(data);
        out.extend_from_slice(&body);
        out.extend_from_slice(&crc32(&body).to_be_bytes());
    };
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &z);
    chunk(b"IEND", &[]);
    out
}
