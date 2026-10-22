// Прототип перехода «сетевой лаг»: пикселизация, плавно восстанавливающая резкость.
// Без зависимостей: rustc -O lag.rs -o lag && ./lag out
// Источник - синтетический кадр (градиент + круг + полосы). Вывод - PNG (stored deflate).
use std::fs;
const W: usize = 160;
const H: usize = 90;

fn src(x: usize, y: usize) -> [u8; 3] {
    let (fx, fy) = (x as f32 / W as f32, y as f32 / H as f32);
    let mut c = [(40.0 + 120.0 * fx) as u8, (60.0 + 100.0 * fy) as u8, 140u8];
    let (dx, dy) = (x as f32 - 80.0, y as f32 - 45.0);
    if dx * dx + dy * dy < 24.0 * 24.0 {
        c = [230, 200, 160];
    }
    if y > 70 && (x / 5) % 2 == 0 {
        c = [20, 20, 20];
    }
    c
}

// block: размер блока в пикселях (1 = резко). Блок усредняется, как при реальном даунскейле.
fn render(block: usize) -> Vec<u8> {
    let mut out = vec![0u8; W * H * 3];
    let mut by = 0;
    while by < H {
        let mut bx = 0;
        while bx < W {
            let (mut s, mut n) = ([0u32; 3], 0u32);
            for y in by..(by + block).min(H) {
                for x in bx..(bx + block).min(W) {
                    let p = src(x, y);
                    for i in 0..3 {
                        s[i] += p[i] as u32;
                    }
                    n += 1;
                }
            }
            for y in by..(by + block).min(H) {
                for x in bx..(bx + block).min(W) {
                    for i in 0..3 {
                        out[(y * W + x) * 3 + i] = (s[i] / n) as u8;
                    }
                }
            }
            bx += block;
        }
        by += block;
    }
    out
}

fn crc32(d: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in d {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
    }
    !c
}

fn chunk(o: &mut Vec<u8>, t: &[u8; 4], d: &[u8]) {
    o.extend((d.len() as u32).to_be_bytes());
    let mut b = t.to_vec();
    b.extend(d);
    o.extend(&b);
    o.extend(crc32(&b).to_be_bytes());
}

fn png(rgb: &[u8]) -> Vec<u8> {
    let mut raw = Vec::new();
    for y in 0..H {
        raw.push(0);
        raw.extend(&rgb[y * W * 3..(y + 1) * W * 3]);
    }
    let mut z = vec![0x78, 0x01];
    let mut i = 0;
    while i < raw.len() {
        let n = (raw.len() - i).min(65535);
        z.push(if i + n == raw.len() { 1 } else { 0 });
        z.extend((n as u16).to_le_bytes());
        z.extend((!(n as u16)).to_le_bytes());
        z.extend(&raw[i..i + n]);
        i += n;
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &raw {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    z.extend(((b << 16) | a).to_be_bytes());
    let mut o = vec![0x89, b'P', b'N', b'G', 13, 10, 26, 10];
    let mut h = Vec::new();
    h.extend((W as u32).to_be_bytes());
    h.extend((H as u32).to_be_bytes());
    h.extend([8, 2, 0, 0, 0]);
    chunk(&mut o, b"IHDR", &h);
    chunk(&mut o, b"IDAT", &z);
    chunk(&mut o, b"IEND", &[]);
    o
}

fn main() {
    let dir = std::env::args().nth(1).unwrap_or("out".into());
    fs::create_dir_all(&dir).unwrap();
    // В реальности 2.5 с при 30 fps = 75 кадров; здесь пишем 8 ключевых.
    // Размер блока 48 -> 1, экспоненциально.
    let frames = 8;
    for k in 0..frames {
        let t = k as f32 / (frames - 1) as f32;
        let e = t; // линейно в лог-шкале блока = ровное визуальное восстановление
        let block = ((48.0f32).powf(1.0 - e)).round().max(1.0) as usize;
        fs::write(format!("{dir}/lag_{k:02}_b{block}.png"), png(&render(block))).unwrap();
        println!("frame {k}: block {block}px");
    }
}
