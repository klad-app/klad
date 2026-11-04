use egui::Vec2;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub center: Vec2,
    pub zoom: f32,
    pub angle: f32,
    pub flip: bool,
}

fn rotate(v: Vec2, angle: f32) -> Vec2 {
    let (s, c) = angle.sin_cos();
    Vec2::new(v.x * c - v.y * s, v.x * s + v.y * c)
}

fn mirror(v: Vec2, flip: bool) -> Vec2 {
    match flip {
        true => Vec2::new(-v.x, v.y),
        false => v,
    }
}

impl View {
    pub fn fit(canvas: Vec2, area: Vec2) -> Self {
        Self {
            center: canvas / 2.0,
            zoom: (area.x / canvas.x).min(area.y / canvas.y) * 0.92,
            angle: 0.0,
            flip: false,
        }
    }

    pub fn to_screen(self, canvas: Vec2, area: Vec2) -> Vec2 {
        rotate(mirror(canvas - self.center, self.flip), self.angle) * self.zoom + area / 2.0
    }

    pub fn to_canvas(self, screen: Vec2, area: Vec2) -> Vec2 {
        mirror(rotate((screen - area / 2.0) / self.zoom, -self.angle), self.flip) + self.center
    }

    pub fn pan(&mut self, delta: Vec2) {
        self.center -= mirror(rotate(delta / self.zoom, -self.angle), self.flip);
    }

    fn keep(&mut self, anchor: Vec2, at: Vec2, area: Vec2) {
        self.center = anchor - mirror(rotate((at - area / 2.0) / self.zoom, -self.angle), self.flip);
    }

    pub fn flip_at(&mut self, at: Vec2, area: Vec2) {
        let anchor = self.to_canvas(at, area);
        self.flip = !self.flip;
        self.keep(anchor, at, area);
    }

    pub fn zoom_at(&mut self, factor: f32, at: Vec2, area: Vec2) {
        let anchor = self.to_canvas(at, area);
        self.zoom = (self.zoom * factor).clamp(0.02, 64.0);
        self.keep(anchor, at, area);
    }

    pub fn rotate_at(&mut self, delta: f32, at: Vec2, area: Vec2) {
        let anchor = self.to_canvas(at, area);
        self.angle += delta;
        self.keep(anchor, at, area);
    }

    pub fn uniform(self, area: Vec2, pixels_per_point: f32, canvas: Vec2) -> [f32; 12] {
        let (s, c) = self.angle.sin_cos();
        let k = pixels_per_point * self.zoom;
        let f = if self.flip { -1.0 } else { 1.0 };
        let (m00, m01, m10, m11) = (k * c * f, -k * s, k * s * f, k * c);
        let t = area * pixels_per_point / 2.0
            - Vec2::new(
                m00 * self.center.x + m01 * self.center.y,
                m10 * self.center.x + m11 * self.center.y,
            );
        [
            m00,
            m01,
            m10,
            m11,
            t.x,
            t.y,
            area.x * pixels_per_point,
            area.y * pixels_per_point,
            canvas.x,
            canvas.y,
            pixels_per_point,
            8.0 * pixels_per_point,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec2, b: Vec2) -> bool {
        (a - b).length() < 1e-3
    }

    #[test]
    fn screen_and_canvas_round_trip() {
        let area = Vec2::new(1200.0, 800.0);
        let mut view = View::fit(Vec2::new(4096.0, 2048.0), area);
        view.angle = 0.7;
        let p = Vec2::new(321.0, 654.0);
        assert!(close(view.to_canvas(view.to_screen(p, area), area), p));
    }

    #[test]
    fn zoom_and_rotate_keep_the_anchor() {
        let area = Vec2::new(1000.0, 700.0);
        let mut view = View::fit(Vec2::new(2048.0, 2048.0), area);
        let at = Vec2::new(250.0, 600.0);
        let anchor = view.to_canvas(at, area);
        view.zoom_at(3.0, at, area);
        view.rotate_at(0.4, at, area);
        assert!(close(view.to_canvas(at, area), anchor));
    }

    #[test]
    fn uniform_matches_to_screen() {
        let area = Vec2::new(900.0, 600.0);
        let mut view = View::fit(Vec2::new(1024.0, 1024.0), area);
        view.angle = -0.3;
        let u = view.uniform(area, 2.0, Vec2::new(1024.0, 1024.0));
        let p = Vec2::new(100.0, 700.0);
        let px = Vec2::new(u[0] * p.x + u[1] * p.y + u[4], u[2] * p.x + u[3] * p.y + u[5]);
        assert!(close(px, view.to_screen(p, area) * 2.0));
    }

    #[test]
    fn a_flipped_view_mirrors_and_still_maps_both_ways() {
        let area = Vec2::new(900.0, 600.0);
        let mut view = View::fit(Vec2::new(1000.0, 800.0), area);
        let left = Vec2::new(100.0, 400.0);
        let right = Vec2::new(900.0, 400.0);
        let (l, r) = (view.to_screen(left, area), view.to_screen(right, area));
        assert!(l.x < r.x);
        view.flip_at(area / 2.0, area);
        assert!(view.to_screen(left, area).x > view.to_screen(right, area).x);
        assert!(close(view.to_screen(left, area), Vec2::new(r.x, l.y)));
        view.angle = 0.4;
        view.zoom_at(1.7, Vec2::new(200.0, 100.0), area);
        let p = Vec2::new(321.0, 654.0);
        assert!(close(view.to_canvas(view.to_screen(p, area), area), p));
        let u = view.uniform(area, 2.0, Vec2::new(1000.0, 800.0));
        let px = Vec2::new(u[0] * p.x + u[1] * p.y + u[4], u[2] * p.x + u[3] * p.y + u[5]);
        assert!(close(px, view.to_screen(p, area) * 2.0));
        let before = view.to_screen(p, area);
        view.pan(Vec2::new(30.0, -12.0));
        assert!(close(view.to_screen(p, area), before + Vec2::new(30.0, -12.0)));
        let pen = Vec2::new(420.0, 233.0);
        let anchor = view.to_canvas(pen, area);
        view.flip_at(pen, area);
        assert!(close(view.to_canvas(pen, area), anchor));
    }

    #[test]
    fn pan_moves_content_with_the_pointer() {
        let area = Vec2::new(800.0, 600.0);
        let mut view = View::fit(Vec2::new(1000.0, 1000.0), area);
        view.angle = 1.1;
        let p = Vec2::new(500.0, 500.0);
        let before = view.to_screen(p, area);
        view.pan(Vec2::new(30.0, -12.0));
        assert!(close(view.to_screen(p, area), before + Vec2::new(30.0, -12.0)));
    }
}
