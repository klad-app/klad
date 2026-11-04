use crate::color::parse_hex;
use crate::doc::{Blend, Paint, Stop};
use crate::rng::hash2_unit;
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effect {
    Haze(Haze),
    Blur(Blur),
    Glow(Glow),
    Outline(Outline),
    Shadow(Shadow),
    Noise(Noise),
    Displace(Displace),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Haze {
    pub amount: f32,
    pub color: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top: Option<String>,
    pub span: f32,
    pub desaturate: f32,
}

impl Default for Haze {
    fn default() -> Self {
        Self {
            amount: 0.4,
            color: "#9aa8b8".into(),
            top: None,
            span: 0.0,
            desaturate: 0.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Atmosphere {
    pub color: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top: Option<String>,
    pub density: f32,
    pub blur: f32,
    pub desaturate: f32,
}

impl Default for Atmosphere {
    fn default() -> Self {
        Self {
            color: "#9aa8b8".into(),
            top: None,
            density: 0.5,
            blur: 1.0,
            desaturate: 0.15,
        }
    }
}

impl Atmosphere {
    pub fn validate(&self) -> Result<()> {
        parse_hex(&self.color)?;
        self.top.as_deref().map_or(Ok(()), |t| parse_hex(t).map(|_| ()))?;
        if self.density < 0.0 || self.blur < 0.0 || self.desaturate < 0.0 {
            bail!("atmosphere: density, blur and desaturate are 0 or more");
        }
        Ok(())
    }

    pub fn effects(&self, depth: f32, height: f32) -> Vec<Effect> {
        if depth <= 0.0 {
            return Vec::new();
        }
        let haze = Effect::Haze(Haze {
            amount: 1.0 - libm::expf(-self.density * depth),
            color: self.color.clone(),
            top: self.top.clone(),
            span: height,
            desaturate: (self.desaturate * depth).min(1.0),
        });
        let sigma = self.blur * depth;
        std::iter::once(haze)
            .chain((sigma >= 0.25).then_some(Effect::Blur(Blur { radius: sigma })))
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Blur {
    pub radius: f32,
}

impl Default for Blur {
    fn default() -> Self {
        Self { radius: 4.0 }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Glow {
    pub radius: f32,
    pub strength: f32,
    pub threshold: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    pub blend: Blend,
}

impl Default for Glow {
    fn default() -> Self {
        Self {
            radius: 12.0,
            strength: 1.0,
            threshold: 0.0,
            color: None,
            blend: Blend::Screen,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Outline {
    pub width: f32,
    pub color: String,
    #[serde(skip_serializing_if = "is_false")]
    pub inside: bool,
}

impl Default for Outline {
    fn default() -> Self {
        Self {
            width: 3.0,
            color: "#1b1b1b".into(),
            inside: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Shadow {
    pub x: f32,
    pub y: f32,
    pub radius: f32,
    pub color: String,
}

impl Default for Shadow {
    fn default() -> Self {
        Self {
            x: 6.0,
            y: 8.0,
            radius: 6.0,
            color: "#00000080".into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Noise {
    pub amount: f32,
    pub size: f32,
    pub seed: u32,
    pub mono: bool,
}

impl Default for Noise {
    fn default() -> Self {
        Self {
            amount: 0.08,
            size: 1.0,
            seed: 1,
            mono: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Displace {
    pub amount: f32,
    pub size: f32,
    pub seed: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub by: Option<String>,
}

impl Default for Displace {
    fn default() -> Self {
        Self {
            amount: 8.0,
            size: 48.0,
            seed: 1,
            by: None,
        }
    }
}

fn is_false(v: &bool) -> bool {
    !*v
}

pub const EFFECTS: [&str; 7] = ["blur", "glow", "outline", "shadow", "noise", "displace", "haze"];

impl Effect {
    pub fn named(name: &str) -> Option<Self> {
        Some(match name {
            "blur" => Effect::Blur(Blur::default()),
            "glow" => Effect::Glow(Glow::default()),
            "outline" => Effect::Outline(Outline::default()),
            "shadow" => Effect::Shadow(Shadow::default()),
            "noise" => Effect::Noise(Noise::default()),
            "displace" => Effect::Displace(Displace::default()),
            "haze" => Effect::Haze(Haze::default()),
            _ => return None,
        })
    }

    pub fn name(&self) -> &'static str {
        match self {
            Effect::Blur(_) => "blur",
            Effect::Glow(_) => "glow",
            Effect::Outline(_) => "outline",
            Effect::Shadow(_) => "shadow",
            Effect::Noise(_) => "noise",
            Effect::Displace(_) => "displace",
            Effect::Haze(_) => "haze",
        }
    }

    pub fn reach(&self) -> f32 {
        match self {
            Effect::Blur(b) => b.radius.max(0.0) * 3.0,
            Effect::Glow(g) => g.radius.max(0.0) * 3.0,
            Effect::Outline(o) => o.width.max(0.0) + 1.0,
            Effect::Shadow(s) => s.x.abs().max(s.y.abs()) + s.radius.max(0.0) * 3.0,
            Effect::Noise(_) | Effect::Haze(_) => 0.0,
            Effect::Displace(d) => d.amount.abs() + 1.0,
        }
    }

    pub fn validate(&self) -> Result<()> {
        let colour = |c: &str| parse_hex(c).map(|_| ());
        match self {
            Effect::Blur(b) if !(0.0..=256.0).contains(&b.radius) => {
                bail!("blur radius {} is outside 0..256", b.radius)
            }
            Effect::Glow(g) if !(0.0..=256.0).contains(&g.radius) => {
                bail!("glow radius {} is outside 0..256", g.radius)
            }
            Effect::Glow(g) => g.color.as_deref().map_or(Ok(()), colour),
            Effect::Outline(o) if !(0.0..=128.0).contains(&o.width) => {
                bail!("outline width {} is outside 0..128", o.width)
            }
            Effect::Outline(o) => colour(&o.color),
            Effect::Shadow(s) if !(0.0..=256.0).contains(&s.radius) => {
                bail!("shadow radius {} is outside 0..256", s.radius)
            }
            Effect::Shadow(s) if s.x.abs().max(s.y.abs()) > 1024.0 => bail!("shadow offset is over 1024 px"),
            Effect::Shadow(s) => colour(&s.color),
            Effect::Noise(n) if n.size <= 0.0 => bail!("noise size must be above 0"),
            Effect::Displace(d) if d.size <= 0.0 => bail!("displacement size must be above 0"),
            Effect::Displace(d) if d.amount.abs() > 512.0 => bail!("displacement is over 512 px"),
            Effect::Haze(h) => {
                colour(&h.color)?;
                h.top.as_deref().map_or(Ok(()), colour)
            }
            _ => Ok(()),
        }
    }
}

pub fn reach(effects: &[Effect]) -> f32 {
    effects.iter().map(Effect::reach).sum()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Adjustment {
    Hsl(Hsl),
    Levels(Levels),
    Curves(Curves),
    Balance(Balance),
    GradientMap(GradientMap),
    Fill(ColorFill),
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Hsl {
    pub hue: f32,
    pub saturation: f32,
    pub lightness: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Levels {
    pub black: f32,
    pub white: f32,
    pub gamma: f32,
    pub low: f32,
    pub high: f32,
}

impl Default for Levels {
    fn default() -> Self {
        Self {
            black: 0.0,
            white: 1.0,
            gamma: 1.0,
            low: 0.0,
            high: 1.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Curves {
    pub points: Vec<[f32; 2]>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub red: Vec<[f32; 2]>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub green: Vec<[f32; 2]>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub blue: Vec<[f32; 2]>,
}

impl Default for Curves {
    fn default() -> Self {
        Self {
            points: vec![[0.0, 0.0], [1.0, 1.0]],
            red: Vec::new(),
            green: Vec::new(),
            blue: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct Balance {
    pub shadows: [f32; 3],
    pub midtones: [f32; 3],
    pub highlights: [f32; 3],
    pub keep_light: bool,
}

impl Default for Balance {
    fn default() -> Self {
        Self {
            shadows: [0.0; 3],
            midtones: [0.0; 3],
            highlights: [0.0; 3],
            keep_light: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GradientMap {
    pub stops: Vec<Stop>,
}

impl Default for GradientMap {
    fn default() -> Self {
        Self {
            stops: vec![Stop(0.0, "#1a1033".into()), Stop(1.0, "#ffe9b0".into())],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ColorFill {
    pub paint: Paint,
}

impl Default for ColorFill {
    fn default() -> Self {
        Self {
            paint: Paint::Solid("#203060".into()),
        }
    }
}

pub const ADJUSTMENTS: [&str; 6] = ["hsl", "levels", "curves", "balance", "gradient-map", "fill"];

impl Adjustment {
    pub fn named(name: &str) -> Option<Self> {
        Some(match name {
            "hsl" => Adjustment::Hsl(Hsl::default()),
            "levels" => Adjustment::Levels(Levels::default()),
            "curves" => Adjustment::Curves(Curves::default()),
            "balance" => Adjustment::Balance(Balance::default()),
            "gradient-map" => Adjustment::GradientMap(GradientMap::default()),
            "fill" => Adjustment::Fill(ColorFill::default()),
            _ => return None,
        })
    }

    pub fn name(&self) -> &'static str {
        match self {
            Adjustment::Hsl(_) => "hsl",
            Adjustment::Levels(_) => "levels",
            Adjustment::Curves(_) => "curves",
            Adjustment::Balance(_) => "balance",
            Adjustment::GradientMap(_) => "gradient-map",
            Adjustment::Fill(_) => "fill",
        }
    }

    pub fn label(&self) -> &'static str {
        label_of(self.name())
    }

    pub fn reads_under(&self) -> bool {
        !matches!(self, Adjustment::Fill(_))
    }

    pub fn validate(&self) -> Result<()> {
        match self {
            Adjustment::Fill(f) => match &f.paint {
                Paint::Solid(c) => parse_hex(c).map(|_| ()),
                Paint::Linear { linear } => linear.stops.iter().try_for_each(|s| parse_hex(&s.1).map(|_| ())),
                Paint::Radial { radial } => radial.stops.iter().try_for_each(|s| parse_hex(&s.1).map(|_| ())),
            },
            other => other.prepare().map(|_| ()),
        }
    }

    pub fn prepare(&self) -> Result<Prepared> {
        Ok(match self {
            Adjustment::Hsl(h) => Prepared::Hsl(h.clone()),
            Adjustment::Levels(l) => {
                if l.white <= l.black {
                    bail!("levels: white {} must be above black {}", l.white, l.black);
                }
                if l.gamma <= 0.0 {
                    bail!("levels: gamma must be above 0");
                }
                Prepared::Levels(l.clone())
            }
            Adjustment::Curves(c) => Prepared::Curves(Box::new([
                curve_lut(&c.points)?,
                curve_lut(&c.red)?,
                curve_lut(&c.green)?,
                curve_lut(&c.blue)?,
            ])),
            Adjustment::Balance(b) => Prepared::Balance(b.clone()),
            Adjustment::GradientMap(g) => {
                if g.stops.is_empty() {
                    bail!("a gradient map needs at least one stop");
                }
                let mut stops: Vec<(f32, [f32; 3])> = g
                    .stops
                    .iter()
                    .map(|s| parse_hex(&s.1).map(|c| (s.0, [c[0], c[1], c[2]])))
                    .collect::<Result<_>>()?;
                stops.sort_by(|a, b| a.0.total_cmp(&b.0));
                Prepared::Map(stops)
            }
            Adjustment::Fill(_) => bail!("a colour fill is drawn, not applied to pixels"),
        })
    }
}

pub fn label_of(name: &str) -> &'static str {
    match name {
        "hsl" => "hue and saturation",
        "levels" => "levels",
        "curves" => "curves",
        "balance" => "colour balance",
        "gradient-map" => "gradient map",
        "fill" => "colour fill",
        _ => "adjustment",
    }
}

const LUT: usize = 1024;

pub enum Prepared {
    Hsl(Hsl),
    Levels(Levels),
    Curves(Box<[Option<Vec<f32>>; 4]>),
    Balance(Balance),
    Map(Vec<(f32, [f32; 3])>),
}

impl Prepared {
    pub fn apply(&self, rgb: [f32; 3]) -> [f32; 3] {
        match self {
            Prepared::Hsl(h) => hsl_shift(rgb, h),
            Prepared::Levels(l) => rgb.map(|c| {
                let v = ((c - l.black) / (l.white - l.black)).clamp(0.0, 1.0);
                l.low + libm::powf(v, 1.0 / l.gamma) * (l.high - l.low)
            }),
            Prepared::Curves(luts) => {
                let [master, r, g, b] = &**luts;
                let through = |lut: &Option<Vec<f32>>, v: f32| lut.as_deref().map_or(v, |t| sample(t, v));
                let [cr, cg, cb] = [through(r, rgb[0]), through(g, rgb[1]), through(b, rgb[2])];
                [through(master, cr), through(master, cg), through(master, cb)]
            }
            Prepared::Balance(b) => balance(rgb, b),
            Prepared::Map(stops) => {
                let l = luma(rgb).clamp(0.0, 1.0);
                gradient_at(stops, l)
            }
        }
    }
}

pub fn luma(rgb: [f32; 3]) -> f32 {
    0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2]
}

fn gradient_at(stops: &[(f32, [f32; 3])], t: f32) -> [f32; 3] {
    let after = stops.iter().position(|s| s.0 > t);
    match after {
        None => stops[stops.len() - 1].1,
        Some(0) => stops[0].1,
        Some(i) => {
            let (a, b) = (stops[i - 1], stops[i]);
            let k = (t - a.0) / (b.0 - a.0);
            [0, 1, 2].map(|c| a.1[c] + (b.1[c] - a.1[c]) * k)
        }
    }
}

fn sample(lut: &[f32], v: f32) -> f32 {
    let x = v.clamp(0.0, 1.0) * (LUT - 1) as f32;
    let i = (x as usize).min(LUT - 2);
    let f = x - i as f32;
    lut[i] + (lut[i + 1] - lut[i]) * f
}

fn curve_lut(points: &[[f32; 2]]) -> Result<Option<Vec<f32>>> {
    if points.is_empty() {
        return Ok(None);
    }
    let mut pts: Vec<[f32; 2]> = points.to_vec();
    if pts.iter().flatten().any(|v| !v.is_finite()) {
        bail!("curve points must be numbers");
    }
    pts.sort_by(|a, b| a[0].total_cmp(&b[0]));
    pts.dedup_by(|b, a| (b[0] - a[0]).abs() < 1e-4);
    if pts.len() == 1 {
        return Ok(Some(vec![pts[0][1]; LUT]));
    }
    let slopes: Vec<f32> = pts
        .windows(2)
        .map(|w| (w[1][1] - w[0][1]) / (w[1][0] - w[0][0]))
        .collect();
    let n = pts.len();
    let mut tangents: Vec<f32> = (0..n)
        .map(|k| match k {
            0 => slopes[0],
            k if k == n - 1 => slopes[n - 2],
            k if slopes[k - 1] * slopes[k] <= 0.0 => 0.0,
            k => (slopes[k - 1] + slopes[k]) / 2.0,
        })
        .collect();
    (0..n - 1).for_each(|k| {
        if slopes[k] == 0.0 {
            tangents[k] = 0.0;
            tangents[k + 1] = 0.0;
            return;
        }
        let (a, b) = (tangents[k] / slopes[k], tangents[k + 1] / slopes[k]);
        let h = a * a + b * b;
        if h > 9.0 {
            let t = 3.0 / libm::sqrtf(h);
            tangents[k] = t * a * slopes[k];
            tangents[k + 1] = t * b * slopes[k];
        }
    });
    let eval = |x: f32| -> f32 {
        if x <= pts[0][0] {
            return pts[0][1];
        }
        if x >= pts[n - 1][0] {
            return pts[n - 1][1];
        }
        let k = pts.windows(2).position(|w| x < w[1][0]).unwrap_or(n - 2);
        let (x0, y0, x1, y1) = (pts[k][0], pts[k][1], pts[k + 1][0], pts[k + 1][1]);
        let h = x1 - x0;
        let t = (x - x0) / h;
        let (t2, t3) = (t * t, t * t * t);
        (2.0 * t3 - 3.0 * t2 + 1.0) * y0
            + (t3 - 2.0 * t2 + t) * h * tangents[k]
            + (-2.0 * t3 + 3.0 * t2) * y1
            + (t3 - t2) * h * tangents[k + 1]
    };
    Ok(Some((0..LUT).map(|i| eval(i as f32 / (LUT - 1) as f32)).collect()))
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn balance(rgb: [f32; 3], b: &Balance) -> [f32; 3] {
    let l = luma(rgb);
    let shadows = 1.0 - smoothstep(0.0, 0.55, l);
    let highlights = smoothstep(0.45, 1.0, l);
    let mids = (1.0 - shadows - highlights).max(0.0);
    let shifted: [f32; 3] = [0, 1, 2].map(|c| {
        let delta = b.shadows[c] * shadows + b.midtones[c] * mids + b.highlights[c] * highlights;
        (rgb[c] + delta * 0.5).clamp(0.0, 1.0)
    });
    match b.keep_light {
        true => set_lum(shifted, l),
        false => shifted,
    }
}

fn rgb_to_hsl([r, g, b]: [f32; 3]) -> [f32; 3] {
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let l = (max + min) / 2.0;
    let d = max - min;
    if d <= 1e-6 {
        return [0.0, 0.0, l];
    }
    let s = match l > 0.5 {
        true => d / (2.0 - max - min),
        false => d / (max + min),
    };
    let h = match max {
        m if m == r => (g - b) / d + if g < b { 6.0 } else { 0.0 },
        m if m == g => (b - r) / d + 2.0,
        _ => (r - g) / d + 4.0,
    };
    [h / 6.0, s, l]
}

fn hsl_to_rgb([h, s, l]: [f32; 3]) -> [f32; 3] {
    if s <= 0.0 {
        return [l, l, l];
    }
    let q = match l < 0.5 {
        true => l * (1.0 + s),
        false => l + s - l * s,
    };
    let p = 2.0 * l - q;
    let channel = |t: f32| {
        let t = t.rem_euclid(1.0);
        match t {
            t if t < 1.0 / 6.0 => p + (q - p) * 6.0 * t,
            t if t < 0.5 => q,
            t if t < 2.0 / 3.0 => p + (q - p) * (2.0 / 3.0 - t) * 6.0,
            _ => p,
        }
    };
    [channel(h + 1.0 / 3.0), channel(h), channel(h - 1.0 / 3.0)]
}

fn hsl_shift(rgb: [f32; 3], adjust: &Hsl) -> [f32; 3] {
    let [h, s, l] = rgb_to_hsl(rgb.map(|c| c.clamp(0.0, 1.0)));
    let s = (s * (1.0 + adjust.saturation.clamp(-1.0, 4.0))).clamp(0.0, 1.0);
    let lift = adjust.lightness.clamp(-1.0, 1.0);
    let l = match lift >= 0.0 {
        true => l + (1.0 - l) * lift,
        false => l * (1.0 + lift),
    };
    hsl_to_rgb([h + adjust.hue / 360.0, s, l])
}

fn lum(rgb: [f32; 3]) -> f32 {
    0.3 * rgb[0] + 0.59 * rgb[1] + 0.11 * rgb[2]
}

fn clip_color(rgb: [f32; 3]) -> [f32; 3] {
    let l = lum(rgb);
    let (n, x) = (rgb[0].min(rgb[1]).min(rgb[2]), rgb[0].max(rgb[1]).max(rgb[2]));
    let low = match n < 0.0 && l - n > 1e-9 {
        true => rgb.map(|c| l + (c - l) * l / (l - n)),
        false => rgb,
    };
    match x > 1.0 && x - l > 1e-9 {
        true => low.map(|c| l + (c - l) * (1.0 - l) / (x - l)),
        false => low,
    }
}

fn set_lum(rgb: [f32; 3], l: f32) -> [f32; 3] {
    let d = l - lum(rgb);
    clip_color(rgb.map(|c| c + d))
}

pub fn blend_rgb(mode: Blend, b: [f32; 3], s: [f32; 3]) -> [f32; 3] {
    let each = |f: fn(f32, f32) -> f32| [f(b[0], s[0]), f(b[1], s[1]), f(b[2], s[2])];
    let screen = |b: f32, s: f32| b + s - b * s;
    let hard = |b: f32, s: f32| match s <= 0.5 {
        true => b * 2.0 * s,
        false => {
            let t = 2.0 * s - 1.0;
            b + t - b * t
        }
    };
    match mode {
        Blend::Normal => s,
        Blend::Multiply => each(|b, s| b * s),
        Blend::Screen => [screen(b[0], s[0]), screen(b[1], s[1]), screen(b[2], s[2])],
        Blend::Overlay => [hard(s[0], b[0]), hard(s[1], b[1]), hard(s[2], b[2])],
        Blend::Add => each(|b, s| (b + s).min(1.0)),
        Blend::Darken => each(f32::min),
        Blend::Lighten => each(f32::max),
        Blend::ColorDodge => each(|b, s| match (b, s) {
            (b, _) if b <= 0.0 => 0.0,
            (_, s) if s >= 1.0 => 1.0,
            (b, s) => (b / (1.0 - s)).min(1.0),
        }),
        Blend::HardLight => [hard(b[0], s[0]), hard(b[1], s[1]), hard(b[2], s[2])],
        Blend::SoftLight => each(|b, s| match s <= 0.5 {
            true => b - (1.0 - 2.0 * s) * b * (1.0 - b),
            false => {
                let d = match b <= 0.25 {
                    true => ((16.0 * b - 12.0) * b + 4.0) * b,
                    false => libm::sqrtf(b),
                };
                b + (2.0 * s - 1.0) * (d - b)
            }
        }),
        Blend::Color => set_lum(s, lum(b)),
        Blend::Luminosity => set_lum(b, lum(s)),
    }
}

pub fn composite(mode: Blend, backdrop: [f32; 4], source: [f32; 4]) -> [f32; 4] {
    let (ab, a_s) = (backdrop[3], source[3]);
    let un = |p: [f32; 4]| match p[3] > 0.0 {
        true => [p[0] / p[3], p[1] / p[3], p[2] / p[3]],
        false => [0.0; 3],
    };
    let (cb, cs) = (un(backdrop), un(source));
    if mode == Blend::Add {
        return [0, 1, 2, 3].map(|c| (backdrop[c] + source[c]).min(1.0));
    }
    let mixed = blend_rgb(mode, cb, cs);
    let alpha = a_s + ab * (1.0 - a_s);
    let rgb = [0, 1, 2].map(|c| a_s * ((1.0 - ab) * cs[c] + ab * mixed[c]) + (1.0 - a_s) * backdrop[c]);
    [rgb[0], rgb[1], rgb[2], alpha]
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Rays {
    pub x: f32,
    pub y: f32,
    pub angle: f32,
    pub spread: f32,
    pub count: u32,
    pub length: f32,
    pub start: f32,
    pub falloff: f32,
    pub noise: f32,
    pub seed: u32,
}

impl Default for Rays {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            angle: 90.0,
            spread: 40.0,
            count: 7,
            length: 600.0,
            start: 0.0,
            falloff: 1.0,
            noise: 0.5,
            seed: 1,
        }
    }
}

pub struct Beam {
    pub quad: [[f32; 2]; 4],
    pub length: f32,
    pub strength: f32,
}

impl Rays {
    pub fn validate(&self) -> Result<()> {
        if self.count == 0 || self.count > 256 {
            bail!("rays: count {} is outside 1..256", self.count);
        }
        if self.length <= 0.0 {
            bail!("rays: length must be above 0");
        }
        if !(0.0..=360.0).contains(&self.spread) {
            bail!("rays: spread {} is outside 0..360 degrees", self.spread);
        }
        Ok(())
    }

    pub fn beams(&self) -> Vec<Beam> {
        let count = self.count.clamp(1, 256);
        let step = self.spread / count as f32;
        let noise = self.noise.clamp(0.0, 1.0);
        let seed = self.seed;
        (0..count)
            .map(|i| {
                let r = |k: i32| hash2_unit(i as i32, k, seed ^ 0x5241_5953);
                let middle = self.angle - self.spread / 2.0 + step * (i as f32 + 0.5) + (r(1) - 0.5) * step * noise;
                let width = step * (0.35 + 0.5 * r(2) * noise + 0.5 * (1.0 - noise));
                let length = self.length * (1.0 - 0.6 * noise * r(3));
                let strength = 1.0 - 0.75 * noise * r(4);
                let dir = |deg: f32| {
                    let a = deg.to_radians();
                    [libm::cosf(a), libm::sinf(a)]
                };
                let across = dir(middle + 90.0);
                let half = self.start / 2.0 / count as f32 * (0.6 + 0.8 * r(5));
                let foot = [
                    self.x + across[0] * (r(6) - 0.5) * self.start,
                    self.y + across[1] * (r(6) - 0.5) * self.start,
                ];
                let (a, b) = (dir(middle - width / 2.0), dir(middle + width / 2.0));
                Beam {
                    quad: [
                        [foot[0] - across[0] * half, foot[1] - across[1] * half],
                        [
                            foot[0] - across[0] * half + a[0] * length,
                            foot[1] - across[1] * half + a[1] * length,
                        ],
                        [
                            foot[0] + across[0] * half + b[0] * length,
                            foot[1] + across[1] * half + b[1] * length,
                        ],
                        [foot[0] + across[0] * half, foot[1] + across[1] * half],
                    ],
                    length,
                    strength,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1e-4)
    }

    #[test]
    fn effects_read_with_defaults_and_say_how_far_they_reach() {
        let list: Vec<Effect> =
            serde_json::from_str(r#"[{"glow":{}},{"outline":{"width":4}},{"shadow":{"x":-3,"y":10,"radius":2}}]"#)
                .unwrap();
        assert_eq!(list[0], Effect::Glow(Glow::default()));
        assert_eq!(reach(&list), 36.0 + 5.0 + 16.0);
        assert!(serde_json::from_str::<Effect>(r#"{"glow":{"radious":3}}"#).is_err());
        assert!(
            Effect::Outline(Outline {
                color: "red".into(),
                ..Outline::default()
            })
            .validate()
            .is_err()
        );
        assert_eq!(
            serde_json::to_string(&list[1]).unwrap(),
            r##"{"outline":{"width":4.0,"color":"#1b1b1b"}}"##
        );
        EFFECTS
            .iter()
            .for_each(|n| assert_eq!(Effect::named(n).unwrap().name(), *n));
    }

    #[test]
    fn levels_curves_and_maps_do_what_they_say() {
        let levels = Adjustment::Levels(Levels {
            black: 0.2,
            white: 0.6,
            gamma: 1.0,
            low: 0.0,
            high: 1.0,
        })
        .prepare()
        .unwrap();
        assert!(close(levels.apply([0.1, 0.4, 0.9]), [0.0, 0.5, 1.0]));
        let identity = Adjustment::Curves(Curves::default()).prepare().unwrap();
        assert!(close(identity.apply([0.13, 0.5, 0.77]), [0.13, 0.5, 0.77]));
        let dusk = Adjustment::Curves(Curves {
            points: vec![[0.0, 0.0], [0.5, 0.3], [1.0, 0.8]],
            blue: vec![[0.0, 0.1], [1.0, 1.0]],
            ..Curves::default()
        })
        .prepare()
        .unwrap();
        let out = dusk.apply([0.5, 0.5, 0.0]);
        assert!((out[0] - 0.3).abs() < 1e-3 && (out[1] - 0.3).abs() < 1e-3, "{out:?}");
        assert!(out[2] > 0.0 && out[2] < 0.1, "{out:?}");
        let steep = Adjustment::Curves(Curves {
            points: vec![[0.0, 0.0], [0.4, 0.9], [0.6, 0.95], [1.0, 1.0]],
            ..Curves::default()
        })
        .prepare()
        .unwrap();
        let ys: Vec<f32> = (0..=100).map(|i| steep.apply([i as f32 / 100.0; 3])[0]).collect();
        assert!(ys.windows(2).all(|w| w[1] >= w[0] - 1e-6) && ys.iter().all(|y| *y <= 1.0 + 1e-6));
        let map = Adjustment::GradientMap(GradientMap {
            stops: vec![Stop(0.0, "#000000".into()), Stop(1.0, "#ff0000".into())],
        })
        .prepare()
        .unwrap();
        assert!(close(map.apply([1.0, 1.0, 1.0]), [1.0, 0.0, 0.0]));
        assert!(close(map.apply([0.5, 0.5, 0.5]), [0.5, 0.0, 0.0]));
        assert!(
            Adjustment::Levels(Levels {
                white: 0.1,
                black: 0.2,
                ..Levels::default()
            })
            .prepare()
            .is_err()
        );
    }

    #[test]
    fn hue_turns_and_balance_keeps_the_light() {
        let turn = Adjustment::Hsl(Hsl {
            hue: 120.0,
            ..Hsl::default()
        })
        .prepare()
        .unwrap();
        assert!(close(turn.apply([1.0, 0.0, 0.0]), [0.0, 1.0, 0.0]));
        let grey = Adjustment::Hsl(Hsl {
            saturation: -1.0,
            ..Hsl::default()
        })
        .prepare()
        .unwrap()
        .apply([0.8, 0.2, 0.4]);
        assert!((grey[0] - grey[1]).abs() < 1e-5 && (grey[1] - grey[2]).abs() < 1e-5);
        let warm = Adjustment::Balance(Balance {
            midtones: [0.4, 0.0, -0.4],
            ..Balance::default()
        })
        .prepare()
        .unwrap();
        let out = warm.apply([0.5, 0.5, 0.5]);
        assert!(out[0] > out[1] && out[1] > out[2], "{out:?}");
        assert!((lum(out) - lum([0.5; 3])).abs() < 1e-4);
    }

    #[test]
    fn blend_formulas_meet_their_known_values() {
        let (b, s) = ([0.2, 0.5, 0.8], [0.6, 0.3, 0.9]);
        assert!(close(blend_rgb(Blend::Multiply, b, s), [0.12, 0.15, 0.72]));
        assert!(close(blend_rgb(Blend::Screen, b, s), [0.68, 0.65, 0.98]));
        assert!(close(blend_rgb(Blend::Darken, b, s), [0.2, 0.3, 0.8]));
        assert!(close(blend_rgb(Blend::Lighten, b, s), [0.6, 0.5, 0.9]));
        assert!(close(blend_rgb(Blend::ColorDodge, b, s), [0.5, 0.5 / 0.7, 1.0]));
        assert!(close(blend_rgb(Blend::HardLight, b, s), [0.36, 0.3, 0.96]));
        let lumed = blend_rgb(Blend::Luminosity, b, s);
        assert!((lum(lumed) - lum(s)).abs() < 1e-5);
        assert_eq!(
            composite(Blend::Normal, [0.0, 0.0, 1.0, 1.0], [0.5, 0.0, 0.0, 0.5]),
            [0.5, 0.0, 0.5, 1.0]
        );
    }

    #[test]
    fn rays_repeat_with_their_seed() {
        let rays = Rays {
            x: 100.0,
            y: 50.0,
            ..Rays::default()
        };
        let a: Vec<_> = rays.beams().iter().map(|b| (b.quad, b.strength)).collect();
        let b: Vec<_> = rays.beams().iter().map(|b| (b.quad, b.strength)).collect();
        let c: Vec<_> = Rays {
            seed: 2,
            ..rays.clone()
        }
        .beams()
        .iter()
        .map(|b| (b.quad, b.strength))
        .collect();
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(a.len(), 7);
        assert!(rays.beams().iter().all(|b| b.quad[1][1] > 50.0 && b.length <= 600.0));
    }
}
