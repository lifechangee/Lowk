//! The O4 swatch sheet: ~40 representative backdrops run through the REAL [`AdaptiveTone`], with the contrast of
//! every text block verified per pixel. Used by the unit test below (every swatch must pass, and the committed
//! `crates/lowk-stealth/tests/fixtures/swatches.json` must equal the current output) and by the `overlay_swatches`
//! example that writes the JSON and the HTML sheet (the PNG is rendered from the HTML by the Playwright spec).

use crate::palette::{self, Rgb};
use crate::tone::{AdaptiveTone, SAMPLE_H, SAMPLE_W};
use lowk_types::{Tone, ToneKind, ToneMode};
use std::fmt::Write as _;

/// Text bands per swatch (top / middle / bottom).
pub const BANDS: usize = 3;
/// CSS scrim opacity floor at the default 60 % transparency (`--floor`).
pub const ALPHA: f32 = palette::SCRIM_FLOOR;
/// Thresholds every swatch must reach (per pixel, worst pixel of the block).
pub const MIN_BODY_WCAG: f32 = 4.5;
pub const MIN_BODY_LC: f32 = 60.0;
pub const MIN_MUTED_WCAG: f32 = 3.0;
pub const MIN_ACCENT_WCAG: f32 = 3.0;

pub struct Backdrop {
    pub name: &'static str,
    pub rgba: Vec<u8>,
}

fn px(mut f: impl FnMut(usize, usize) -> Rgb) -> Vec<u8> {
    let mut v = Vec::with_capacity((SAMPLE_W * SAMPLE_H * 4) as usize);
    for y in 0..SAMPLE_H as usize {
        for x in 0..SAMPLE_W as usize {
            let c = f(x, y);
            v.extend_from_slice(&[c[0], c[1], c[2], 255]);
        }
    }
    v
}

fn flat(c: Rgb) -> Vec<u8> {
    px(|_, _| c)
}

fn lerp(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let m = |i: usize| (f32::from(a[i]) + (f32::from(b[i]) - f32::from(a[i])) * t).round() as u8;
    [m(0), m(1), m(2)]
}

/// Deterministic xorshift noise in `[0, 1)`.
struct Rng(u32);
impl Rng {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 >> 8) as f32 / 16_777_216.0
    }
}

fn noise(base: Rgb, spread: f32, seed: u32) -> Vec<u8> {
    let mut r = Rng(seed);
    px(|_, _| {
        let n = (r.next() - 0.5) * 2.0 * spread;
        let m = |v: u8| (f32::from(v) + n * 255.0).clamp(1.0, 255.0) as u8;
        [m(base[0]), m(base[1]), m(base[2])]
    })
}

pub fn all() -> Vec<Backdrop> {
    let w = SAMPLE_W as usize;
    let h = SAMPLE_H as usize;
    let mut v: Vec<Backdrop> = Vec::new();
    let mut add = |name: &'static str, rgba: Vec<u8>| v.push(Backdrop { name, rgba });
    // neutrals
    add("Чистый чёрный", flat([0, 0, 0]));
    add("Почти чёрный", flat([8, 8, 10]));
    add("Графит", flat([34, 36, 42]));
    add("Тёмно-серый", flat([72, 72, 76]));
    add("Средне-серый", flat([128, 128, 128]));
    add("Светло-серый", flat([190, 190, 194]));
    add("Молочный", flat([240, 240, 236]));
    add("Чистый белый", flat([255, 255, 255]));
    // blues
    add("Тёмно-синий (navy)", flat([0, 0, 128]));
    add("Глубокий синий", flat([10, 20, 90]));
    add("Королевский синий", flat([30, 60, 200]));
    add("Чистый синий", flat([0, 0, 255]));
    add("Небесно-голубой", flat([120, 190, 240]));
    add("Светло-голубой", flat([170, 205, 255]));
    // greens / teals
    add("Бирюзовый (teal)", flat([0, 128, 128]));
    add("Циан", flat([0, 230, 240]));
    add("Тёмно-зелёный", flat([10, 70, 30]));
    add("Зелёный", flat([0, 170, 60]));
    add("Лайм", flat([160, 230, 40]));
    add("Оливковый", flat([110, 110, 30]));
    // warm
    add("Жёлтый", flat([255, 235, 40]));
    add("Янтарный", flat([255, 170, 0]));
    add("Оранжевый", flat([240, 110, 20]));
    add("Красный", flat([220, 30, 30]));
    add("Тёмно-красный", flat([110, 10, 20]));
    add("Малиновый", flat([170, 20, 60]));
    add("Розовый", flat([255, 120, 190]));
    // bright saturated colours: light tinted scrim, dark text (design decision)
    add("Жёлтый (замер вживую)", flat([240, 220, 40]));
    add("Чистый жёлтый", flat([255, 255, 0]));
    add("Золотой", flat([255, 215, 0]));
    add("Светло-оранжевый", flat([255, 190, 90]));
    add("Светло-зелёный", flat([144, 238, 144]));
    add("Ярко-зелёный", flat([0, 255, 0]));
    add("Светлый циан", flat([120, 240, 250]));
    // purples / earth
    add("Пурпурный (magenta)", flat([210, 0, 190]));
    add("Фиолетовый", flat([128, 0, 160]));
    add("Глубокий фиолетовый", flat([60, 20, 110]));
    add("Лавандовый", flat([190, 170, 240]));
    add("Коричневый", flat([110, 70, 40]));
    add("Бежевый", flat([235, 220, 190]));
    // gradients and busy content
    add(
        "Градиент navy → циан",
        px(|x, _| lerp([0, 0, 128], [0, 230, 240], x as f32 / (w - 1) as f32)),
    );
    add(
        "Закат: фиолетовый → оранжевый",
        px(|_, y| lerp([70, 20, 130], [250, 140, 30], y as f32 / (h - 1) as f32)),
    );
    add(
        "Градиент чёрный → белый",
        px(|x, y| lerp([4, 4, 4], [255, 255, 255], (x + y) as f32 / (w + h - 2) as f32)),
    );
    add(
        "Код слева, документ справа",
        px(|x, _| if x < w / 2 { [23, 24, 28] } else { [241, 242, 245] }),
    );
    add("Фото: тёплый шум", noise([150, 100, 60], 0.28, 0x2545_f491));
    add("Фото: холодный шум", noise([60, 100, 160], 0.28, 0x9e37_79b9));
    add("Видео: тёмное с яркими пятнами", {
        let mut r = Rng(0x1234_5679);
        px(|_, _| {
            if r.next() > 0.88 {
                [250, 245, 230]
            } else {
                [18, 20, 30]
            }
        })
    });
    add(
        "Мелкий текст на белом",
        px(|x, y| {
            if (x + y * 2) % 3 == 0 {
                [20, 20, 20]
            } else {
                [250, 250, 250]
            }
        }),
    );
    v
}

/// One text block (a row of the overlay) of one swatch.
pub struct BlockResult {
    pub tone: Tone,
    pub scrim: Rgb,
    pub fg: Rgb,
    pub body_wcag: f32,
    pub body_lc: f32,
    pub muted_wcag: f32,
    pub muted_lc: f32,
    pub int_wcag: f32,
    pub mv_wcag: f32,
}

pub struct SwatchResult {
    pub name: &'static str,
    pub rgba: Vec<u8>,
    pub mean: Rgb,
    pub kind: ToneKind,
    pub blocks: Vec<BlockResult>,
}

impl SwatchResult {
    pub fn min_body_wcag(&self) -> f32 {
        self.blocks.iter().map(|b| b.body_wcag).fold(99.0, f32::min)
    }
    pub fn min_body_lc(&self) -> f32 {
        self.blocks.iter().map(|b| b.body_lc).fold(999.0, f32::min)
    }
    pub fn min_muted_wcag(&self) -> f32 {
        self.blocks.iter().map(|b| b.muted_wcag).fold(99.0, f32::min)
    }
    pub fn min_accent_wcag(&self) -> f32 {
        self.blocks
            .iter()
            .map(|b| b.int_wcag.min(b.mv_wcag))
            .fold(99.0, f32::min)
    }
    pub fn ok(&self) -> bool {
        self.min_body_wcag() >= MIN_BODY_WCAG
            && self.min_body_lc() >= MIN_BODY_LC
            && self.min_muted_wcag() >= MIN_MUTED_WCAG
            && self.min_accent_wcag() >= MIN_ACCENT_WCAG
    }
}

fn parse(s: &str) -> Rgb {
    let v: Vec<u8> = s.split(',').map(|x| x.trim().parse().unwrap_or(0)).collect();
    [v[0], v[1], v[2]]
}

/// The tone cells the overlay would receive for this backdrop (settled: the same bitmap fed 4 times).
pub fn run(b: &Backdrop) -> SwatchResult {
    let mut t = AdaptiveTone::new(ToneMode::Auto);
    let mut out = None;
    for _ in 0..4 {
        if let Some(x) = t.update(&b.rgba, SAMPLE_W, SAMPLE_H) {
            out = Some(x);
        }
    }
    // A pure-black sample is "unknown" for the sampler: the overlay keeps its default dark tone.
    let tone = out.unwrap_or_else(|| t.current());
    let grid = tone.grid.clone().expect("the grid always rides on the tone");
    let (cols, rows) = (usize::from(grid.cols), usize::from(grid.rows));
    let (w, h) = (SAMPLE_W as usize, SAMPLE_H as usize);
    let mut blocks = Vec::new();
    // The sheet keeps its three text bands (top / middle / bottom); each spans a third of the grid rows.
    for band in 0..BANDS {
        let (r0, r1) = (band * rows / BANDS, (band + 1) * rows / BANDS);
        // Same rule as the UI's blockTone(): the cell whose scrim lightness is closest to the band average.
        let cells = &grid.cells[r0 * cols..r1 * cols];
        let y = |c: &Tone| palette::rel_luminance(parse(&c.rgb));
        let avg = cells.iter().map(y).sum::<f32>() / cells.len() as f32;
        let best = cells
            .iter()
            .min_by(|a, b| (y(a) - avg).abs().total_cmp(&(y(b) - avg).abs()))
            .unwrap();
        let (scrim, fg, int, mv) = (
            parse(&best.rgb),
            parse(&best.fg),
            parse(&best.int),
            parse(&best.mv),
        );
        let (mut bw, mut bl, mut mw, mut ml, mut iw, mut vw) =
            (99.0f32, 999.0f32, 99.0f32, 999.0f32, 99.0f32, 99.0f32);
        let scheme = palette::Scheme {
            dark_text: false,
            tinted: false,
            scrim,
            fg,
            inv: [0; 3],
            int,
            mv,
        };
        for yy in r0 * h / rows..r1 * h / rows {
            for xx in 0..w {
                let p = &b.rgba[(yy * w + xx) * 4..];
                let v = palette::verdict(&scheme, [p[0], p[1], p[2]], ALPHA);
                bw = bw.min(v.body_wcag);
                bl = bl.min(v.body_lc);
                mw = mw.min(v.muted_wcag);
                ml = ml.min(v.muted_lc);
                iw = iw.min(v.int_wcag);
                vw = vw.min(v.mv_wcag);
            }
        }
        blocks.push(BlockResult {
            tone: best.clone(),
            scrim,
            fg,
            body_wcag: bw,
            body_lc: bl,
            muted_wcag: mw,
            muted_lc: ml,
            int_wcag: iw,
            mv_wcag: vw,
        });
    }
    let n = (w * h) as f32;
    let mut sum = [0f32; 3];
    for p in b.rgba.chunks_exact(4) {
        for i in 0..3 {
            sum[i] += f32::from(p[i]);
        }
    }
    let mean = [(sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8];
    SwatchResult {
        name: b.name,
        rgba: b.rgba.clone(),
        mean,
        kind: tone.tone,
        blocks,
    }
}

pub fn run_all() -> Vec<SwatchResult> {
    all().iter().map(run).collect()
}

fn hex(c: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

/// A SAMPLE_W x SAMPLE_H backdrop as an SVG (horizontal runs merged), used as a CSS `background-image` or inline.
pub fn backdrop_svg(rgba: &[u8]) -> String {
    let (w, h) = (SAMPLE_W as usize, SAMPLE_H as usize);
    let mut s = format!("<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 {w} {h}' preserveAspectRatio='none' shape-rendering='crispEdges'>");
    for y in 0..h {
        let mut x = 0;
        while x < w {
            let c = &rgba[(y * w + x) * 4..(y * w + x) * 4 + 3];
            let mut e = x + 1;
            while e < w && &rgba[(y * w + e) * 4..(y * w + e) * 4 + 3] == c {
                e += 1;
            }
            let _ = write!(
                s,
                "<rect x='{x}' y='{y}' width='{}' height='1' fill='{}'/>",
                e - x,
                hex([c[0], c[1], c[2]])
            );
            x = e;
        }
    }
    s.push_str("</svg>");
    s
}

fn data_uri(svg: &str) -> String {
    let mut o = String::from("data:image/svg+xml;utf8,");
    for c in svg.chars() {
        match c {
            '<' => o.push_str("%3C"),
            '>' => o.push_str("%3E"),
            '#' => o.push_str("%23"),
            '%' => o.push_str("%25"),
            ' ' => o.push_str("%20"),
            c => o.push(c),
        }
    }
    o
}

fn json_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Machine-readable results (the committed `swatches.json`; also feeds the Playwright overlay spec).
pub fn to_json(rs: &[SwatchResult]) -> String {
    let mut s = String::from("[\n");
    for (i, r) in rs.iter().enumerate() {
        let _ = write!(
            s,
            "  {{\"name\":{},\"mean\":{},\"kind\":{},\"ok\":{},\"min_body_wcag\":{:.2},\"min_body_lc\":{:.1},\"min_muted_wcag\":{:.2},\"min_accent_wcag\":{:.2},\"blocks\":[",
            json_str(r.name),
            json_str(&hex(r.mean)),
            json_str(&format!("{:?}", r.kind).to_lowercase()),
            r.ok(),
            r.min_body_wcag(),
            r.min_body_lc(),
            r.min_muted_wcag(),
            r.min_accent_wcag()
        );
        for (j, b) in r.blocks.iter().enumerate() {
            let _ = write!(
                s,
                "{}{{\"scrim\":{},\"fg\":{},\"int\":{},\"mv\":{},\"body_wcag\":{:.2},\"body_lc\":{:.1},\"muted_wcag\":{:.2},\"muted_lc\":{:.1},\"int_wcag\":{:.2},\"mv_wcag\":{:.2}}}",
                if j > 0 { "," } else { "" },
                json_str(&hex(b.scrim)),
                json_str(&hex(b.fg)),
                json_str(&b.tone.int),
                json_str(&b.tone.mv),
                b.body_wcag,
                b.body_lc,
                b.muted_wcag,
                b.muted_lc,
                b.int_wcag,
                b.mv_wcag
            );
        }
        let _ = writeln!(s, "]}}{}", if i + 1 < rs.len() { "," } else { "" });
    }
    s.push(']');
    s.push('\n');
    s
}

/// The self-contained HTML sheet: each card = backdrop + the overlay's three rows (scrim at the CSS floor
/// opacity, body text, muted text, the two accents) + the verified numbers. No scripts, no external files.
pub fn to_html(rs: &[SwatchResult]) -> String {
    let pass = rs.iter().filter(|r| r.ok()).count();
    let mut s = String::new();
    let _ = write!(
        s,
        "<!doctype html><html lang=\"ru\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>Lowk: цветовая база оверлея</title><style>{CSS}</style></head><body><h1>Цветовая база адаптивного оверлея (O4)</h1><p class=\"lead\">{n} фонов прогнаны через настоящий <code>AdaptiveTone</code>. Скрим показан при минимальной непрозрачности {a:.0}% (потолок прозрачности в CSS). Числа считаются по худшему пикселю фона под каждой строкой: WCAG 2.x для основного и приглушённого текста и акцентов, APCA Lc для основного. Порог: основной ≥ {bw} (WCAG) и ≥ {bl} Lc, приглушённый ≥ {mw}, акценты ≥ {aw}. Сейчас проходят {pass} из {n}.</p><div class=\"grid\">",
        n = rs.len(),
        a = ALPHA * 100.0,
        bw = MIN_BODY_WCAG,
        bl = MIN_BODY_LC,
        mw = MIN_MUTED_WCAG,
        aw = MIN_ACCENT_WCAG,
        pass = pass
    );
    let rows = ["Субтитры", "Ответ ИИ", "Мой ответ"];
    for (i, r) in rs.iter().enumerate() {
        let bg = data_uri(&backdrop_svg(&r.rgba));
        let _ = write!(
            s,
            "<section class=\"card\" data-i=\"{i}\" data-ok=\"{}\"><div class=\"stage\"><div class=\"bg\" style=\"background-image:url(&quot;{bg}&quot;)\"></div><div class=\"ov\">",
            u8::from(r.ok())
        );
        for (k, b) in r.blocks.iter().enumerate() {
            let muted_a = palette::MUTED_ALPHA;
            let _ = write!(
                s,
                "<div class=\"row\" style=\"background:rgba({sr},{sg},{sb},{a});color:rgb({fr},{fg},{fb})\"><b>{lbl}</b> Привет, это пример текста · <span style=\"color:rgba({fr},{fg},{fb},{ma})\">приглушённый</span> <i style=\"background:rgb({ir})\"></i><i style=\"background:rgb({mr})\"></i></div>",
                sr = b.scrim[0],
                sg = b.scrim[1],
                sb = b.scrim[2],
                a = ALPHA,
                fr = b.fg[0],
                fg = b.fg[1],
                fb = b.fg[2],
                ma = muted_a,
                lbl = rows[k.min(2)],
                ir = b.tone.int,
                mr = b.tone.mv
            );
        }
        let b0 = &r.blocks[0];
        let _ = write!(
            s,
            "</div></div><h3>{n}<small>средний {mean}</small></h3><table><tr><th>скрим</th><td><i class=\"sw\" style=\"background:{sc}\"></i>{sc}</td><th>текст</th><td><i class=\"sw\" style=\"background:{fc}\"></i>{fc}</td></tr><tr><th>основной</th><td>{bw:.1}:1 · Lc {bl:.0}</td><th>приглушённый</th><td>{mw:.1}:1 · Lc {ml:.0}</td></tr><tr><th>акценты</th><td>{iw:.1}:1 / {vw:.1}:1</td><th>вид</th><td>{kind:?}</td></tr></table><p class=\"verdict {cls}\">{mark}</p></section>",
            n = r.name,
            mean = hex(r.mean),
            sc = hex(b0.scrim),
            fc = hex(b0.fg),
            bw = r.min_body_wcag(),
            bl = r.min_body_lc(),
            mw = r.min_muted_wcag(),
            ml = r.blocks.iter().map(|b| b.muted_lc).fold(999.0, f32::min),
            iw = r.blocks.iter().map(|b| b.int_wcag).fold(99.0, f32::min),
            vw = r.blocks.iter().map(|b| b.mv_wcag).fold(99.0, f32::min),
            kind = r.kind,
            cls = if r.ok() { "ok" } else { "bad" },
            mark = if r.ok() { "проверено ✓" } else { "НЕ ПРОХОДИТ" }
        );
    }
    s.push_str("</div></body></html>\n");
    s
}

const CSS: &str = "*{box-sizing:border-box}body{margin:0;padding:24px 16px;background:#17181c;color:#e8e9ee;font:14px/1.45 -apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif}h1{font-size:22px;margin:0 0 8px}.lead{max-width:960px;margin:0 0 20px;color:#b5b8c4}code{font-family:ui-monospace,Menlo,monospace}.grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(300px,1fr));gap:16px}.card{background:#20222a;border-radius:12px;padding:12px;overflow:hidden}.stage{position:relative;height:150px;border-radius:8px;overflow:hidden}.bg{position:absolute;inset:-12px;background-size:100% 100%;filter:blur(8px)}.ov{position:absolute;inset:8px;display:flex;flex-direction:column;gap:6px;justify-content:center}.row{border-radius:8px;padding:6px 8px;font-size:12px;line-height:16px;white-space:nowrap;overflow:hidden;display:flex;align-items:center;gap:4px}.row b{font-size:10px;letter-spacing:.04em;text-transform:uppercase;margin-right:4px}.row i{display:inline-block;width:10px;height:10px;border-radius:50%;margin-left:4px}h3{margin:10px 0 6px;font-size:14px;display:flex;justify-content:space-between;gap:8px}h3 small{font-weight:400;color:#9a9dac}table{width:100%;border-collapse:collapse;font-size:12px}th{text-align:left;color:#9a9dac;font-weight:400;padding:2px 6px 2px 0}td{padding:2px 8px 2px 0;font-variant-numeric:tabular-nums}.sw{display:inline-block;width:10px;height:10px;border-radius:3px;margin-right:6px;border:1px solid #555;vertical-align:-1px}.verdict{margin:8px 0 0;font-weight:600;font-size:12px}.ok{color:#5ee08a}.bad{color:#ff6b6b}";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forty_backdrops_every_one_legible() {
        let rs = run_all();
        assert!(rs.len() >= 40, "{}", rs.len());
        for r in &rs {
            assert!(
                r.ok(),
                "{}: body {:.2} Lc {:.1} muted {:.2} accents {:.2}",
                r.name,
                r.min_body_wcag(),
                r.min_body_lc(),
                r.min_muted_wcag(),
                r.min_accent_wcag()
            );
        }
    }

    #[test]
    fn bright_saturated_backdrops_get_a_light_scrim_and_dark_text() {
        let rs = run_all();
        for n in [
            "Жёлтый",
            "Жёлтый (замер вживую)",
            "Чистый жёлтый",
            "Золотой",
            "Светло-оранжевый",
            "Светло-зелёный",
            "Ярко-зелёный",
            "Светлый циан",
            "Лайм",
            "Розовый",
        ] {
            let r = rs
                .iter()
                .find(|r| r.name == n)
                .unwrap_or_else(|| panic!("no swatch {n}"));
            assert!(r.ok(), "{n}");
            let sc = r.blocks[1].scrim;
            let lab = crate::palette::rgb_to_oklab(sc);
            assert!(lab.l >= 0.94, "{n}: scrim {sc:?} is not light");
        }
    }

    #[test]
    fn dark_blue_is_blue_and_light_blue_is_pale() {
        let rs = run_all();
        let get = |n: &str| rs.iter().find(|r| r.name.contains(n)).unwrap();
        for n in ["navy", "Глубокий синий", "Королевский", "Чистый синий"]
        {
            let b = &get(n).blocks[1];
            assert_eq!(get(n).kind, ToneKind::Color, "{n}");
            assert!(b.scrim[2] > b.scrim[0] + 15, "{n}: {:?}", b.scrim);
        }
        assert_eq!(get("Светло-голубой").kind, ToneKind::Light);
    }

    /// `crates/lowk-stealth/tests/fixtures/swatches.json` must be the current output (regenerate with the example).
    #[test]
    fn committed_json_matches_the_mapping() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../crates/lowk-stealth/tests/fixtures/swatches.json"
        );
        let want = to_json(&run_all());
        if std::env::var_os("UPDATE_SWATCHES").is_some() {
            std::fs::write(path, &want).unwrap();
        }
        let have = std::fs::read_to_string(path)
            .unwrap_or_default()
            .replace("\r\n", "\n");
        assert_eq!(have, want, "palette changed: run `UPDATE_SWATCHES=1 cargo test -p lowk-stealth swatches` and the example, and re-render the PNG");
    }
}
