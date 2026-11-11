use crate::color::{Rgba, parse_hex, to_hex, value_step};
use crate::doc::Swatch;
use anyhow::{Result, anyhow, bail};

pub struct Shipped {
    pub id: &'static str,
    pub title: &'static str,
    pub by: &'static str,
    pub colors: &'static str,
}

pub const SHIPPED: [Shipped; 9] = [
    Shipped {
        id: "1-bit",
        title: "1-bit",
        by: "",
        colors: "000000 ffffff",
    },
    Shipped {
        id: "gameboy",
        title: "Game Boy (bgb)",
        by: "",
        colors: "081820 346856 88c070 e0f8d0",
    },
    Shipped {
        id: "pico-8",
        title: "PICO-8",
        by: "Lexaloffle Games",
        colors: "000000 1d2b53 7e2553 008751 ab5236 5f574f c2c3c7 fff1e8 ff004d ffa300 ffec27 00e436 29adff 83769c ff77a8 ffccaa",
    },
    Shipped {
        id: "sweetie-16",
        title: "Sweetie 16",
        by: "GrafxKid",
        colors: "1a1c2c 5d275d b13e53 ef7d57 ffcd75 a7f070 38b764 257179 29366f 3b5dc9 41a6f6 73eff7 f4f4f4 94b0c2 566c86 333c57",
    },
    Shipped {
        id: "dawnbringer-16",
        title: "DawnBringer 16",
        by: "DawnBringer",
        colors: "140c1c 442434 30346d 4e4a4e 854c30 346524 d04648 757161 597dce d27d2c 8595a1 6daa2c d2aa99 6dc2ca dad45e deeed6",
    },
    Shipped {
        id: "dawnbringer-32",
        title: "DawnBringer 32",
        by: "DawnBringer",
        colors: "000000 222034 45283c 663931 8f563b df7126 d9a066 eec39a fbf236 99e550 6abe30 37946e 4b692f 524b24 323c39 3f3f74 306082 5b6ee1 639bff 5fcde4 cbdbfc ffffff 9badb7 847e87 696a6a 595652 76428a ac3232 d95763 d77bba 8f974a 8a6f30",
    },
    Shipped {
        id: "endesga-32",
        title: "Endesga 32",
        by: "ENDESGA",
        colors: "be4a2f d77643 ead4aa e4a672 b86f50 733e39 3e2731 a22633 e43b44 f77622 feae34 fee761 63c74d 3e8948 265c42 193c3e 124e89 0099db 2ce8f5 ffffff c0cbdc 8b9bb4 5a6988 3a4466 262b44 181425 ff0044 68386c b55088 f6757a e8b796 c28569",
    },
    Shipped {
        id: "resurrect-64",
        title: "Resurrect 64",
        by: "Kerrie Lake",
        colors: "2e222f 3e3546 625565 966c6c ab947a 694f62 7f708a 9babb2 c7dcd0 ffffff 6e2727 b33831 ea4f36 f57d4a ae2334 e83b3b fb6b1d f79617 f9c22b 7a3045 9e4539 cd683d e6904e fbb954 4c3e24 676633 a2a947 d5e04b fbff86 165a4c 239063 1ebc73 91db69 cddf6c 313638 374e4a 547e64 92a984 b2ba90 0b5e65 0b8a8f 0eaf9b 30e1b9 8ff8e2 323353 484a77 4d65b4 4d9be6 8fd3ff 45293f 6b3e75 905ea9 a884f3 eaaded 753c54 a24b6f cf657f ed8099 831c5d c32454 f04f78 f68181 fca790 fdcbb0",
    },
    Shipped {
        id: "aap-64",
        title: "AAP-64",
        by: "Adigun A. Polack",
        colors: "060608 141013 3b1725 73172d b4202a df3e23 fa6a0a f9a31b ffd541 fffc40 d6f264 9cdb43 59c135 14a02e 1a7a3e 24523b 122020 143464 285cc4 249fde 20d6c7 a6fcdb ffffff fef3c0 fad6b8 f5a097 e86a73 bc4a9b 793a80 403353 242234 221c1a 322b28 71413b bb7547 dba463 f4d29c dae0ea b3b9d1 8b93af 6d758d 4a5462 333941 422433 5b3138 8e5252 ba756a e9b5a3 e3e6ff b9bffb 849be4 588dbe 477d85 23674e 328464 5daf8d 92dcba cdf7e2 e4d2aa c7b08b a08662 796755 5a4e44 423934",
    },
];

fn plain(color: String, name: String) -> Swatch {
    Swatch {
        color,
        name,
        id: String::new(),
        ramp: String::new(),
    }
}

impl Shipped {
    pub fn swatches(&self) -> Vec<Swatch> {
        self.colors
            .split_whitespace()
            .map(|hex| plain(format!("#{hex}"), String::new()))
            .collect()
    }

    pub fn count(&self) -> usize {
        self.colors.split_whitespace().count()
    }
}

pub fn shipped(id: &str) -> Option<&'static Shipped> {
    let wanted = id.trim().to_ascii_lowercase();
    SHIPPED.iter().find(|p| p.id == wanted)
}

pub fn names() -> String {
    SHIPPED.iter().map(|p| p.id).collect::<Vec<_>>().join(", ")
}

fn hex_line(line: &str) -> Option<String> {
    let digits = line.trim().trim_start_matches('#');
    let valid = matches!(digits.len(), 6 | 8) && digits.chars().all(|c| c.is_ascii_hexdigit());
    valid.then(|| format!("#{}", digits.to_ascii_lowercase()))
}

fn gimp(text: &str) -> Result<Vec<Swatch>> {
    text.lines()
        .skip(1)
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter(|line| !line.contains(':') || line.chars().next().is_some_and(|c| c.is_ascii_digit()))
        .map(|line| {
            let mut parts = line.split_whitespace();
            let mut channel = || -> Option<u8> { parts.next()?.parse::<u8>().ok() };
            let (Some(r), Some(g), Some(b)) = (channel(), channel(), channel()) else {
                bail!(
                    "{line:?} is not a line of a GIMP palette: it should start with red, green and blue from 0 to 255"
                );
            };
            let name = parts.collect::<Vec<_>>().join(" ");
            let name = match name.as_str() {
                "Untitled" | "untitled" => String::new(),
                other => other.trim_start_matches('#').to_string(),
            };
            Ok(plain(format!("#{r:02x}{g:02x}{b:02x}"), name))
        })
        .collect()
}

pub fn parse(text: &str) -> Result<Vec<Swatch>> {
    let first = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default();
    let swatches = match first.starts_with("GIMP Palette") {
        true => gimp(text)?,
        false => text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with(';') && !line.starts_with("//"))
            .map(|line| {
                hex_line(line).map(|hex| plain(hex, String::new())).ok_or_else(|| {
                    anyhow!(
                        "{line:?} is not a colour: a palette file is a list of hex colours, one per line (.hex), or a GIMP palette (.gpl)"
                    )
                })
            })
            .collect::<Result<Vec<_>>>()?,
    };
    if swatches.is_empty() {
        bail!("the palette file holds no colours");
    }
    Ok(unnamed_twice(swatches))
}

fn unnamed_twice(swatches: Vec<Swatch>) -> Vec<Swatch> {
    let mut seen = std::collections::BTreeSet::new();
    swatches
        .into_iter()
        .map(|s| match s.name.is_empty() || seen.insert(s.name.clone()) {
            true => s,
            false => Swatch {
                name: String::new(),
                ..s
            },
        })
        .collect()
}

pub const RAMP_STEP: f32 = 0.09;

pub fn ramp(base: Rgba, steps: usize) -> Vec<Rgba> {
    let steps = steps.clamp(2, 16);
    let middle = (steps - 1) as f32 / 2.0;
    (0..steps)
        .map(|i| value_step(base, (i as f32 - middle) * RAMP_STEP).map(|v| v.clamp(0.0, 1.0)))
        .map(|c| [c[0], c[1], c[2], base[3]])
        .collect()
}

pub fn ramp_swatches(name: &str, base: &str, steps: usize) -> Result<Vec<Swatch>> {
    let name = name.trim();
    if name.is_empty() || name.starts_with('#') {
        bail!("a ramp needs a name that does not start with #");
    }
    let colours = ramp(parse_hex(base)?, steps);
    Ok(colours
        .iter()
        .enumerate()
        .map(|(i, c)| Swatch {
            color: to_hex(*c),
            name: format!("{name}-{}", i + 1),
            id: String::new(),
            ramp: name.to_string(),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::to_oklab;

    #[test]
    fn shipped_palettes_hold_valid_colours_and_are_found_by_name() {
        let counts: Vec<(&str, usize)> = SHIPPED.iter().map(|p| (p.id, p.count())).collect();
        assert_eq!(
            counts,
            [
                ("1-bit", 2),
                ("gameboy", 4),
                ("pico-8", 16),
                ("sweetie-16", 16),
                ("dawnbringer-16", 16),
                ("dawnbringer-32", 32),
                ("endesga-32", 32),
                ("resurrect-64", 64),
                ("aap-64", 64),
            ]
        );
        SHIPPED.iter().for_each(|p| {
            let swatches = p.swatches();
            swatches.iter().for_each(|s| {
                parse_hex(&s.color).unwrap_or_else(|e| panic!("{} holds {}: {e}", p.id, s.color));
            });
            let distinct: std::collections::BTreeSet<&str> = swatches.iter().map(|s| s.color.as_str()).collect();
            assert_eq!(distinct.len(), swatches.len(), "{} repeats a colour", p.id);
        });
        assert_eq!(shipped(" PICO-8 ").map(|p| p.title), Some("PICO-8"));
        assert!(shipped("nes").is_none());
        assert!(names().contains("endesga-32"));
    }

    #[test]
    fn hex_lists_and_gimp_palettes_are_read() {
        let hex = parse("1a1c2c\n#5D275D\n\n; a comment\nb13e53ff\n").unwrap();
        let colours: Vec<&str> = hex.iter().map(|s| s.color.as_str()).collect();
        assert_eq!(colours, ["#1a1c2c", "#5d275d", "#b13e53ff"]);
        let gpl = parse(
            "GIMP Palette\nName: Sprite\nColumns: 4\n#\n 26  28  44\tnight\n93 39 93 Untitled\n177 62 83\tnight\n255\t205\t117\n",
        )
        .unwrap();
        let read: Vec<(&str, &str)> = gpl.iter().map(|s| (s.color.as_str(), s.name.as_str())).collect();
        assert_eq!(
            read,
            [("#1a1c2c", "night"), ("#5d275d", ""), ("#b13e53", ""), ("#ffcd75", "")],
            "a name used twice is kept once, so the palette stays valid"
        );
        assert!(parse("not a colour").unwrap_err().to_string().contains("one per line"));
        assert!(
            parse("GIMP Palette\n300 0 0\n")
                .unwrap_err()
                .to_string()
                .contains("0 to 255")
        );
        assert!(parse("\n\n").is_err());
    }

    #[test]
    fn a_ramp_runs_from_dark_and_cool_to_light_and_warm_through_its_base() {
        let made = ramp_swatches("wood", "#8b5a2b", 5).unwrap();
        let names: Vec<&str> = made.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["wood-1", "wood-2", "wood-3", "wood-4", "wood-5"]);
        assert!(made.iter().all(|s| s.ramp == "wood"));
        assert_eq!(made[2].color, "#8b5a2b", "the middle step is the base colour");
        let labs: Vec<[f32; 3]> = made.iter().map(|s| to_oklab(parse_hex(&s.color).unwrap())).collect();
        assert!(
            labs.windows(2).all(|w| w[1][0] > w[0][0]),
            "each step is lighter: {labs:?}"
        );
        assert!(
            labs[4][2] > labs[0][2],
            "and the light end is yellower than the dark end"
        );
        assert_eq!(ramp(parse_hex("#336699").unwrap(), 1).len(), 2);
        assert!(ramp_swatches("#bad", "#336699", 3).is_err());
    }
}
