use anyhow::{Result, bail};

pub type Rgba = [f32; 4];

pub fn parse_hex(text: &str) -> Result<Rgba> {
    let hex = text.trim().trim_start_matches('#');
    let channel = |i: usize| -> Result<f32> { Ok(u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)? as f32 / 255.0) };
    match hex.len() {
        6 => Ok([channel(0)?, channel(1)?, channel(2)?, 1.0]),
        8 => Ok([channel(0)?, channel(1)?, channel(2)?, channel(3)?]),
        _ => bail!("colour {text:?} is not #rrggbb or #rrggbbaa"),
    }
}

pub fn to_u16(c: Rgba) -> [u16; 4] {
    c.map(|v| (v.clamp(0.0, 1.0) * 65535.0 + 0.5) as u16)
}

pub fn from_u16(c: [u16; 4]) -> Rgba {
    c.map(|v| v as f32 / 65535.0)
}

pub fn to_hex(c: Rgba) -> String {
    let [r, g, b, a] = c.map(|v| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8);
    match a {
        255 => format!("#{r:02x}{g:02x}{b:02x}"),
        _ => format!("#{r:02x}{g:02x}{b:02x}{a:02x}"),
    }
}

fn linear(v: f32) -> f32 {
    match v <= 0.04045 {
        true => v / 12.92,
        false => libm::powf((v + 0.055) / 1.055, 2.4),
    }
}

fn encoded(v: f32) -> f32 {
    let v = v.clamp(0.0, 1.0);
    match v <= 0.003_130_8 {
        true => v * 12.92,
        false => 1.055 * libm::powf(v, 1.0 / 2.4) - 0.055,
    }
}

pub fn to_linear(c: Rgba) -> [f32; 3] {
    [linear(c[0]), linear(c[1]), linear(c[2])]
}

pub fn from_linear(rgb: [f32; 3], alpha: f32) -> Rgba {
    [encoded(rgb[0]), encoded(rgb[1]), encoded(rgb[2]), alpha]
}

pub fn to_oklab(c: Rgba) -> [f32; 3] {
    let [r, g, b] = [linear(c[0]), linear(c[1]), linear(c[2])];
    let l = libm::cbrtf(0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b);
    let m = libm::cbrtf(0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b);
    let s = libm::cbrtf(0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b);
    [
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}

pub fn from_oklab(lab: [f32; 3], alpha: f32) -> Rgba {
    let [l, a, b] = lab;
    let cube = |v: f32| v * v * v;
    let l_ = cube(l + 0.396_337_78 * a + 0.215_803_76 * b);
    let m_ = cube(l - 0.105_561_346 * a - 0.063_854_17 * b);
    let s_ = cube(l - 0.089_484_18 * a - 1.291_485_5 * b);
    [
        encoded(4.076_741_7 * l_ - 3.307_711_6 * m_ + 0.230_969_94 * s_),
        encoded(-1.268_438 * l_ + 2.609_757_4 * m_ - 0.341_319_38 * s_),
        encoded(-0.004_196_086_3 * l_ - 0.703_418_6 * m_ + 1.707_614_7 * s_),
        alpha,
    ]
}

pub fn value_step(c: Rgba, by: f32) -> Rgba {
    let [l, a, b] = to_oklab(c);
    from_oklab([(l + by).clamp(0.0, 1.0), a + by * 0.03, b + by * 0.15], c[3])
}

pub fn recolour(colours: &[Rgba], target: Rgba) -> Vec<Rgba> {
    let n = colours.len().max(1) as f32;
    let labs: Vec<[f32; 4]> = colours
        .iter()
        .map(|c| {
            let [l, a, b] = to_oklab(*c);
            [l, a, b, c[3]]
        })
        .collect();
    let mean = labs.iter().fold([0.0; 4], |m, v| [0, 1, 2, 3].map(|i| m[i] + v[i] / n));
    let [gl, ga, gb] = to_oklab(target);
    let goal = [gl, ga, gb, target[3]];
    labs.iter()
        .map(|v| {
            let [l, a, b, alpha] = [0, 1, 2, 3].map(|i| goal[i] + v[i] - mean[i]);
            from_oklab([l.clamp(0.0, 1.0), a, b], alpha.clamp(0.0, 1.0)).map(|c| c.clamp(0.0, 1.0))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_recolour_moves_the_group_and_keeps_each_shade() {
        let greens = ["#3f6a3a", "#79b85a", "#a8dc84"].map(|h| parse_hex(h).unwrap());
        let amber = parse_hex("#e3962f").unwrap();
        let moved = recolour(&greens, amber);
        let light = |c: &Rgba| to_oklab(*c)[0];
        assert!(
            light(&moved[0]) < light(&moved[1]) && light(&moved[1]) < light(&moved[2]),
            "the shades keep their order"
        );
        let mean = moved
            .iter()
            .map(|c| to_oklab(*c))
            .fold([0.0; 3], |m, l| [0, 1, 2].map(|i| m[i] + l[i] / 3.0));
        let goal = to_oklab(amber);
        assert!(
            (0..3).all(|i| (mean[i] - goal[i]).abs() < 0.02),
            "the group's middle lands on the colour: {mean:?} {goal:?}"
        );
        assert!(moved.iter().all(|c| to_oklab(*c)[2] > 0.05), "every stroke turned warm");
        assert_eq!(
            to_hex(recolour(&greens[..1], amber)[0]),
            "#e3962f",
            "one mark takes the colour as it is"
        );
    }

    #[test]
    fn parses_short_and_alpha_forms() {
        assert_eq!(parse_hex("#ff0000").unwrap(), [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(parse_hex("00ff0080").unwrap()[3], 128.0 / 255.0);
        assert!(parse_hex("#fff").is_err());
    }

    #[test]
    fn oklab_round_trips_and_value_steps_go_darker_cooler_and_lighter_warmer() {
        let c = parse_hex("#6a8f5c").unwrap();
        assert_eq!(to_hex(from_oklab(to_oklab(c), 1.0)), "#6a8f5c");
        let (dark, light) = (value_step(c, -0.16), value_step(c, 0.14));
        assert!(to_oklab(dark)[0] < to_oklab(c)[0] && to_oklab(light)[0] > to_oklab(c)[0]);
        assert!(to_oklab(dark)[2] < to_oklab(c)[2] && to_oklab(light)[2] > to_oklab(c)[2]);
        assert_eq!(to_hex([1.0, 0.0, 0.0, 0.5]), "#ff000080");
    }
}
