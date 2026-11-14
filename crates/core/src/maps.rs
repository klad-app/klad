use crate::color::parse_hex;
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Profile {
    #[default]
    Round,
    Chamfer,
    Pillow,
    Flat,
}

impl Profile {
    pub const ALL: [Profile; 4] = [Profile::Round, Profile::Chamfer, Profile::Pillow, Profile::Flat];

    pub fn name(self) -> &'static str {
        match self {
            Profile::Round => "round",
            Profile::Chamfer => "chamfer",
            Profile::Pillow => "pillow",
            Profile::Flat => "flat",
        }
    }

    pub fn rise(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Profile::Round => libm::sqrtf(1.0 - (1.0 - t) * (1.0 - t)),
            Profile::Chamfer => t,
            Profile::Pillow => t * t * (3.0 - 2.0 * t),
            Profile::Flat => 0.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Surface {
    pub bevel: f32,
    pub profile: Profile,
    pub strength: f32,
    pub body: f32,
    pub range: f32,
}

impl Default for Surface {
    fn default() -> Self {
        Self {
            bevel: 8.0,
            profile: Profile::Round,
            strength: 1.0,
            body: 1.0,
            range: 12.0,
        }
    }
}

impl Surface {
    pub fn validate(&self) -> Result<()> {
        [
            ("bevel", self.bevel, 512.0),
            ("strength", self.strength, 10.0),
            ("body", self.body, 10.0),
            ("range", self.range, 512.0),
        ]
        .iter()
        .try_for_each(|(name, value, most)| {
            if !value.is_finite() || *value < 0.0 || value > most {
                bail!("surface: {name} {value} is outside 0..{most}");
            }
            Ok(())
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Height,
    Normal,
    Specular,
    Emission,
}

impl Role {
    pub const ALL: [Role; 4] = [Role::Height, Role::Normal, Role::Specular, Role::Emission];

    pub fn name(self) -> &'static str {
        match self {
            Role::Height => "height",
            Role::Normal => "normal",
            Role::Specular => "specular",
            Role::Emission => "emission",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LightKind {
    #[default]
    Point,
    Directional,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Light {
    pub kind: LightKind,
    pub at: [f32; 2],
    pub height: f32,
    pub color: String,
    pub energy: f32,
    pub range: f32,
}

impl Default for Light {
    fn default() -> Self {
        Self {
            kind: LightKind::Point,
            at: [0.0, 0.0],
            height: 100.0,
            color: "#ffffff".into(),
            energy: 1.0,
            range: 600.0,
        }
    }
}

impl Light {
    pub fn directional(toward: [f32; 2], height: f32) -> Self {
        Self {
            kind: LightKind::Directional,
            at: toward,
            height,
            ..Self::default()
        }
    }

    pub fn validate(&self) -> Result<()> {
        parse_hex(&self.color)?;
        if !self.at.iter().all(|v| v.is_finite()) {
            bail!("light: at must be two numbers");
        }
        if !self.energy.is_finite() || self.energy < 0.0 {
            bail!("light: energy {} must be 0 or more", self.energy);
        }
        match self.kind {
            LightKind::Point => {
                if !self.height.is_finite() || self.height < 0.0 {
                    bail!("light: height {} must be 0 px or more", self.height);
                }
                if !self.range.is_finite() || self.range <= 0.0 {
                    bail!("light: range {} must be more than 0 px", self.range);
                }
            }
            LightKind::Directional => {
                if !(0.0..=1.0).contains(&self.height) {
                    bail!(
                        "light: a directional light's height {} is from 0 (along the picture) to 1 (from straight in front)",
                        self.height
                    );
                }
                if self.at == [0.0, 0.0] {
                    bail!("light: a directional light needs the direction it comes from, at x,y");
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Maps {
    pub surface: Surface,
    pub ambient: String,
    pub shininess: f32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub lights: Vec<Light>,
}

impl Default for Maps {
    fn default() -> Self {
        Self {
            surface: Surface::default(),
            ambient: "#3a3a46".into(),
            shininess: 24.0,
            lights: Vec::new(),
        }
    }
}

impl Maps {
    pub fn validate(&self) -> Result<()> {
        self.surface.validate()?;
        parse_hex(&self.ambient).map_err(|e| anyhow::anyhow!("maps: ambient: {e}"))?;
        if !self.shininess.is_finite() || !(1.0..=512.0).contains(&self.shininess) {
            bail!("maps: shininess {} is outside 1..512", self.shininess);
        }
        self.lights
            .iter()
            .enumerate()
            .try_for_each(|(i, l)| l.validate().map_err(|e| anyhow::anyhow!("light {}: {e}", i + 1)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_rise_from_the_edge_to_the_top() {
        Profile::ALL.iter().filter(|p| **p != Profile::Flat).for_each(|p| {
            assert_eq!(p.rise(0.0), 0.0, "{}", p.name());
            assert!((p.rise(1.0) - 1.0).abs() < 1e-6, "{}", p.name());
            assert!(p.rise(0.3) < p.rise(0.6), "{} rises", p.name());
        });
        assert!(
            Profile::Round.rise(0.2) > Profile::Chamfer.rise(0.2),
            "a round rim is steep at the edge"
        );
        assert!(
            Profile::Pillow.rise(0.2) < Profile::Chamfer.rise(0.2),
            "a pillow starts soft"
        );
        assert_eq!(Profile::Flat.rise(0.7), 0.0);
    }

    #[test]
    fn maps_read_from_json_with_defaults_and_refuse_nonsense() {
        let maps: Maps = serde_json::from_str(r##"{"surface": {"bevel": 4}, "lights": [{"at": [10, 20]}]}"##).unwrap();
        assert_eq!(maps.surface.bevel, 4.0);
        assert_eq!(maps.surface.profile, Profile::Round);
        assert_eq!(maps.lights[0].kind, LightKind::Point);
        maps.validate().unwrap();
        assert!(
            serde_json::from_str::<Maps>(r#"{"glow": 1}"#).is_err(),
            "unknown keys are refused"
        );
        let bad = Maps {
            lights: vec![Light::directional([0.0, 0.0], 0.5)],
            ..Maps::default()
        };
        assert!(bad.validate().unwrap_err().to_string().contains("direction"));
        let steep = Maps {
            surface: Surface {
                bevel: -1.0,
                ..Surface::default()
            },
            ..Maps::default()
        };
        assert!(steep.validate().unwrap_err().to_string().contains("bevel"));
        assert_eq!(serde_json::to_value(Role::Height).unwrap(), "height");
    }
}
