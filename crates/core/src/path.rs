use anyhow::{Result, bail};

type Point = [f32; 2];

pub(crate) fn tokens(d: &str) -> Result<Vec<Token>> {
    let mut out = Vec::new();
    let chars: Vec<char> = d.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_ascii_alphabetic() && c != 'e' && c != 'E' {
            out.push(Token::Command(c));
            i += 1;
        } else if c.is_ascii_digit() || c == '-' || c == '+' || c == '.' {
            let start = i;
            let mut seen_dot = false;
            let mut seen_exp = false;
            i += 1;
            if c == '.' {
                seen_dot = true;
            }
            while i < chars.len() {
                let n = chars[i];
                let after_exp = matches!(chars[i - 1], 'e' | 'E');
                if n.is_ascii_digit() || ((n == '-' || n == '+') && after_exp) {
                    i += 1;
                } else if n == '.' && !seen_dot && !seen_exp {
                    seen_dot = true;
                    i += 1;
                } else if (n == 'e' || n == 'E') && !seen_exp {
                    seen_exp = true;
                    i += 1;
                } else {
                    break;
                }
            }
            let text: String = chars[start..i].iter().collect();
            match text.parse::<f32>() {
                Ok(v) => out.push(Token::Number(v)),
                Err(_) => bail!("bad number {text:?} in path"),
            }
        } else if c.is_whitespace() || c == ',' {
            i += 1;
        } else {
            bail!("unexpected {c:?} in path");
        }
    }
    Ok(out)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Token {
    Command(char),
    Number(f32),
}

fn cubic(out: &mut Vec<Point>, p0: Point, c1: Point, c2: Point, p3: Point) {
    let hull = dist(p0, c1) + dist(c1, c2) + dist(c2, p3);
    let n = (hull / 3.0).ceil().clamp(2.0, 200.0) as usize;
    (1..=n).for_each(|i| {
        let t = i as f32 / n as f32;
        let u = 1.0 - t;
        let (a, b, c, e) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
        out.push([
            a * p0[0] + b * c1[0] + c * c2[0] + e * p3[0],
            a * p0[1] + b * c1[1] + c * c2[1] + e * p3[1],
        ]);
    });
}

fn quad(out: &mut Vec<Point>, p0: Point, c: Point, p2: Point) {
    let c1 = [p0[0] + (c[0] - p0[0]) * 2.0 / 3.0, p0[1] + (c[1] - p0[1]) * 2.0 / 3.0];
    let c2 = [p2[0] + (c[0] - p2[0]) * 2.0 / 3.0, p2[1] + (c[1] - p2[1]) * 2.0 / 3.0];
    cubic(out, p0, c1, c2, p2);
}

fn dist(a: Point, b: Point) -> f32 {
    libm::sqrtf((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2))
}

pub fn flatten(d: &str) -> Result<Vec<Vec<Point>>> {
    let tokens = tokens(d)?;
    let mut paths: Vec<Vec<Point>> = Vec::new();
    let mut current: Vec<Point> = Vec::new();
    let mut pos: Point = [0.0, 0.0];
    let mut start: Point = [0.0, 0.0];
    let mut last_control: Option<Point> = None;
    let mut command = ' ';
    let mut i = 0;
    let take = |i: &mut usize, n: usize| -> Result<Vec<f32>> {
        let values: Vec<f32> = tokens
            .get(*i..*i + n)
            .unwrap_or(&[])
            .iter()
            .map_while(|t| match t {
                Token::Number(v) => Some(*v),
                Token::Command(_) => None,
            })
            .collect();
        if values.len() != n {
            bail!("path command needs {n} numbers");
        }
        *i += n;
        Ok(values)
    };
    while i < tokens.len() {
        if let Token::Command(c) = tokens[i] {
            command = c;
            i += 1;
            if matches!(c, 'Z' | 'z') {
                if !current.is_empty() {
                    current.push(start);
                    paths.push(std::mem::take(&mut current));
                }
                pos = start;
                last_control = None;
                continue;
            }
        }
        if command == ' ' {
            bail!("path must start with a command");
        }
        if !matches!(command, 'M' | 'm') && current.is_empty() {
            bail!("path must start with M");
        }
        let rel = command.is_ascii_lowercase();
        let at = |p: Point, v: f32, w: f32| if rel { [p[0] + v, p[1] + w] } else { [v, w] };
        match command.to_ascii_uppercase() {
            'M' => {
                let v = take(&mut i, 2)?;
                if current.len() > 1 {
                    paths.push(std::mem::take(&mut current));
                }
                current.clear();
                pos = at(pos, v[0], v[1]);
                start = pos;
                current.push(pos);
                command = if rel { 'l' } else { 'L' };
                last_control = None;
            }
            'L' => {
                let v = take(&mut i, 2)?;
                pos = at(pos, v[0], v[1]);
                current.push(pos);
                last_control = None;
            }
            'H' => {
                let v = take(&mut i, 1)?;
                pos = [if rel { pos[0] + v[0] } else { v[0] }, pos[1]];
                current.push(pos);
                last_control = None;
            }
            'V' => {
                let v = take(&mut i, 1)?;
                pos = [pos[0], if rel { pos[1] + v[0] } else { v[0] }];
                current.push(pos);
                last_control = None;
            }
            'C' => {
                let v = take(&mut i, 6)?;
                let (c1, c2, end) = (at(pos, v[0], v[1]), at(pos, v[2], v[3]), at(pos, v[4], v[5]));
                cubic(&mut current, pos, c1, c2, end);
                last_control = Some(c2);
                pos = end;
            }
            'S' => {
                let v = take(&mut i, 4)?;
                let c1 = last_control.map_or(pos, |c| [2.0 * pos[0] - c[0], 2.0 * pos[1] - c[1]]);
                let (c2, end) = (at(pos, v[0], v[1]), at(pos, v[2], v[3]));
                cubic(&mut current, pos, c1, c2, end);
                last_control = Some(c2);
                pos = end;
            }
            'Q' => {
                let v = take(&mut i, 4)?;
                let (c, end) = (at(pos, v[0], v[1]), at(pos, v[2], v[3]));
                quad(&mut current, pos, c, end);
                last_control = Some(c);
                pos = end;
            }
            'T' => {
                let v = take(&mut i, 2)?;
                let c = last_control.map_or(pos, |c| [2.0 * pos[0] - c[0], 2.0 * pos[1] - c[1]]);
                let end = at(pos, v[0], v[1]);
                quad(&mut current, pos, c, end);
                last_control = Some(c);
                pos = end;
            }
            'A' => bail!("arcs are not supported in stroke paths; use C curves"),
            other => bail!("unknown path command {other:?}"),
        }
    }
    if !current.is_empty() {
        paths.push(current);
    }
    Ok(paths)
}

pub fn length(points: &[Point]) -> f32 {
    points.windows(2).map(|w| dist(w[0], w[1])).sum()
}

pub fn resample(points: &[Point], step: f32) -> Vec<Point> {
    let Some(&first) = points.first() else {
        return Vec::new();
    };
    let mut out = vec![first];
    let mut carry = 0.0f32;
    points.windows(2).for_each(|w| {
        let (a, b) = (w[0], w[1]);
        let seg = dist(a, b);
        let mut t = step - carry;
        while t <= seg {
            let k = t / seg;
            out.push([a[0] + (b[0] - a[0]) * k, a[1] + (b[1] - a[1]) * k]);
            t += step;
        }
        carry = seg - (t - step);
    });
    let last = *points.last().unwrap_or(&first);
    if out.last().is_none_or(|p| dist(*p, last) > step * 0.25) {
        out.push(last);
    }
    out
}

pub fn profile(values: &[f32], t: f32) -> f32 {
    match values {
        [] => 1.0,
        [v] => *v,
        _ => {
            let x = t.clamp(0.0, 1.0) * (values.len() - 1) as f32;
            let i = (x as usize).min(values.len() - 2);
            values[i] + (values[i + 1] - values[i]) * (x - i as f32)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: Point, b: Point) -> bool {
        dist(a, b) < 0.01
    }

    #[test]
    fn lines_relative_and_closed_paths_flatten() {
        let paths = flatten("M10 10 L20 10 l0 10 H5 v-5 Z m 100 0 h10").unwrap();
        assert_eq!(paths.len(), 2);
        let first = &paths[0];
        assert!(near(first[1], [20.0, 10.0]));
        assert!(near(first[2], [20.0, 20.0]));
        assert!(near(first[3], [5.0, 20.0]));
        assert!(near(first[4], [5.0, 15.0]));
        assert!(near(*first.last().unwrap(), [10.0, 10.0]));
        assert!(near(paths[1][0], [110.0, 10.0]));
        assert!(near(paths[1][1], [120.0, 10.0]));
    }

    #[test]
    fn curves_pass_through_their_ends() {
        let paths = flatten("M0 0 C 0 100 100 100 100 0 S 200 -100 200 0 Q 250 50 300 0 T 400 0").unwrap();
        let line = &paths[0];
        assert!(near(*line.last().unwrap(), [400.0, 0.0]));
        assert!(line.iter().any(|p| near(*p, [100.0, 0.0])));
        assert!(line.iter().any(|p| p[1] > 70.0));
        assert!(line.iter().any(|p| p[1] < -70.0));
    }

    #[test]
    fn compact_numbers_and_errors() {
        let paths = flatten("M1.5.5L-2-3e1").unwrap();
        assert!(near(paths[0][0], [1.5, 0.5]));
        assert!(near(paths[0][1], [-2.0, -30.0]));
        assert!(flatten("L1 2").is_err());
        assert!(flatten("M0 0 A 5 5 0 0 1 10 10").is_err());
        assert!(flatten("M0 0 L5").is_err());
    }

    #[test]
    fn resampling_spaces_points_evenly() {
        let points = resample(&[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]], 2.5);
        assert_eq!(points.len(), 9);
        assert!(points.windows(2).all(|w| (dist(w[0], w[1]) - 2.5).abs() < 0.01));
        assert!((length(&points) - 20.0).abs() < 0.01);
    }

    #[test]
    fn profiles_interpolate() {
        assert_eq!(profile(&[], 0.3), 1.0);
        assert_eq!(profile(&[0.4], 0.9), 0.4);
        assert!((profile(&[0.2, 1.0, 0.0], 0.25) - 0.6).abs() < 1e-6);
        assert_eq!(profile(&[0.2, 1.0, 0.0], 1.0), 0.0);
    }
}
