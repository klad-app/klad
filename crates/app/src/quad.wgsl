struct Quad {
    view_a: vec4<f32>,
    view_b: vec4<f32>,
    drag_a: vec4<f32>,
    drag_b: vec4<f32>,
    rect: vec4<f32>,
};

@group(0) @binding(0) var<uniform> q: Quad;
@group(0) @binding(1) var tex: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;

struct Out {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> Out {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0), vec2<f32>(0.0, 1.0),
    );
    let uv = corners[index];
    let p = q.rect.xy + uv * q.rect.zw;
    let d = vec2<f32>(
        q.drag_a.x * p.x + q.drag_a.y * p.y + q.drag_b.x,
        q.drag_a.z * p.x + q.drag_a.w * p.y + q.drag_b.y,
    );
    let px = vec2<f32>(
        q.view_a.x * d.x + q.view_a.y * d.y + q.view_b.x,
        q.view_a.z * d.x + q.view_a.w * d.y + q.view_b.y,
    );
    var out: Out;
    out.pos = vec4<f32>(px.x / q.view_b.z * 2.0 - 1.0, 1.0 - px.y / q.view_b.w * 2.0, 0.0, 1.0);
    out.uv = uv;
    return out;
}

@fragment
fn fs_main(in: Out) -> @location(0) vec4<f32> {
    return textureSample(tex, samp, in.uv) * q.drag_b.z;
}
