pub mod brushes;
pub mod bundle;
pub mod color;
pub mod contact;
pub mod cut;
pub mod doc;
pub mod fill;
pub mod generators;
pub mod here;
pub mod history;
pub mod keys;
pub mod look;
pub mod markedit;
pub mod migrate;
pub mod notes;
pub mod ops;
pub mod patch;
pub mod path;
pub mod pathedit;
pub mod repair;
pub mod retime;
pub mod rig;
pub mod rng;
pub mod samples;
pub mod stroke;
pub mod synth;
pub mod timing;
pub mod tween;
mod varint;

pub mod notices {
    pub const LICENSE: &str = include_str!("../../../LICENSE");
    pub const NOTICES: &str = include_str!("../../../THIRD-PARTY-NOTICES.md");

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn the_licence_and_the_notices_ship_with_the_code() {
            assert!(LICENSE.contains("PolyForm Internal Use License 1.0.0") && LICENSE.contains("Indicode BV"));
            assert!(LICENSE.contains("100,000 euros"), "larger companies buy a licence");
            [
                "skia-safe",
                "eframe",
                "zstd",
                "### Skia",
                "### Phosphor icons",
                "SIL Open Font License",
            ]
            .iter()
            .for_each(|name| assert!(NOTICES.contains(name), "{name}"));
        }
    }
}
