use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const PROPS: [&str; 6] = ["x", "y", "rotate", "sx", "sy", "opacity"];

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Ease {
    #[default]
    Linear,
    In,
    Out,
    InOut,
    Step,
    Curve([f32; 4]),
}

impl Ease {
    pub fn parse(text: &str) -> Result<Self> {
        Ok(match text.trim() {
            "linear" => Ease::Linear,
            "in" => Ease::In,
            "out" => Ease::Out,
            "in-out" => Ease::InOut,
            "step" => Ease::Step,
            other => {
                let n: Vec<f32> = other.split(',').filter_map(|v| v.trim().parse().ok()).collect();
                match n.as_slice() {
                    [a, b, c, d] => Ease::Curve([*a, *b, *c, *d]),
                    _ => bail!("ease {other:?}: linear, in, out, in-out, step, or a curve x1,y1,x2,y2"),
                }
            }
        })
    }

    pub fn at(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Ease::Linear => t,
            Ease::In => t * t,
            Ease::Out => t * (2.0 - t),
            Ease::InOut => t * t * (3.0 - 2.0 * t),
            Ease::Step => {
                if t < 1.0 {
                    0.0
                } else {
                    1.0
                }
            }
            Ease::Curve([x1, y1, x2, y2]) => {
                let bez = |a: f32, b: f32, u: f32| {
                    3.0 * a * u * (1.0 - u) * (1.0 - u) + 3.0 * b * u * u * (1.0 - u) + u * u * u
                };
                let (mut lo, mut hi) = (0.0f32, 1.0f32);
                (0..30).for_each(|_| {
                    let mid = (lo + hi) / 2.0;
                    if bez(x1, x2, mid) < t {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                });
                bez(y1, y2, (lo + hi) / 2.0)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Key {
    pub frame: u32,
    pub value: f32,
    #[serde(default, skip_serializing_if = "is_linear")]
    pub ease: Ease,
}

fn is_linear(e: &Ease) -> bool {
    *e == Ease::Linear
}

pub type Keys = BTreeMap<String, Vec<Key>>;

pub fn value(keys: &Keys, prop: &str, frame: u32) -> Option<f32> {
    let list = keys.get(prop).filter(|l| !l.is_empty())?;
    let after = list.iter().position(|k| k.frame > frame);
    Some(match after {
        Some(0) => list[0].value,
        None => list[list.len() - 1].value,
        Some(i) => {
            let (a, b) = (list[i - 1], list[i]);
            let t = (frame - a.frame) as f32 / (b.frame - a.frame).max(1) as f32;
            a.value + (b.value - a.value) * a.ease.at(t)
        }
    })
}

pub fn set(keys: &mut Keys, prop: &str, key: Key) -> Result<()> {
    if !PROPS.contains(&prop) {
        bail!("no keyable property {prop:?}: {}", PROPS.join(", "));
    }
    let list = keys.entry(prop.to_string()).or_default();
    list.retain(|k| k.frame != key.frame);
    let at = list.iter().position(|k| k.frame > key.frame).unwrap_or(list.len());
    list.insert(at, key);
    Ok(())
}

pub fn remove(keys: &mut Keys, prop: Option<&str>, frame: u32) {
    keys.iter_mut()
        .filter(|(p, _)| prop.is_none_or(|q| q == p.as_str()))
        .for_each(|(_, list)| list.retain(|k| k.frame != frame));
    keys.retain(|_, list| !list.is_empty());
}

pub fn frames(keys: &Keys) -> Vec<u32> {
    let mut all: Vec<u32> = keys.values().flatten().map(|k| k.frame).collect();
    all.sort_unstable();
    all.dedup();
    all
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_interpolate_with_their_ease_and_hold_outside() {
        let mut keys = Keys::new();
        set(
            &mut keys,
            "x",
            Key {
                frame: 0,
                value: 0.0,
                ease: Ease::Linear,
            },
        )
        .unwrap();
        set(
            &mut keys,
            "x",
            Key {
                frame: 10,
                value: 100.0,
                ease: Ease::Out,
            },
        )
        .unwrap();
        set(
            &mut keys,
            "x",
            Key {
                frame: 20,
                value: 0.0,
                ease: Ease::Step,
            },
        )
        .unwrap();
        assert_eq!(value(&keys, "x", 5), Some(50.0));
        assert!(
            (value(&keys, "x", 12).unwrap() - 64.0).abs() < 1e-3,
            "out eases 10 to 20 from 100 towards 0"
        );
        assert_eq!(value(&keys, "x", 30), Some(0.0));
        assert_eq!(value(&keys, "y", 5), None);
        assert!(Ease::In.at(0.5) < 0.5 && Ease::Out.at(0.5) > 0.5 && Ease::InOut.at(0.5) == 0.5);
        assert_eq!(Ease::Step.at(0.99), 0.0);
        let curve = Ease::parse("0.42,0,0.58,1").unwrap();
        assert!((curve.at(0.5) - 0.5).abs() < 0.01 && curve.at(0.2) < 0.2);
        assert!(
            set(
                &mut keys,
                "colour",
                Key {
                    frame: 1,
                    value: 1.0,
                    ease: Ease::Linear
                }
            )
            .is_err()
        );
        remove(&mut keys, None, 10);
        assert_eq!(frames(&keys), [0, 20]);
    }
}
