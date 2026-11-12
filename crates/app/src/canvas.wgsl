struct View {
    a: vec4<f32>,
    b: vec4<f32>,
    c: vec4<f32>,
};

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var canvas_tex: texture_2d<f32>;
@group(0) @binding(2) var canvas_sampler: sampler;

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
    let around = step(0.5, view.c.z);
    let uv = corners[index] * (1.0 + 2.0 * around) - vec2<f32>(around);
    let p = uv * view.c.xy;
    let px = vec2<f32>(
        view.a.x * p.x + view.a.y * p.y + view.b.x,
        view.a.z * p.x + view.a.w * p.y + view.b.y,
    );
    var out: Out;
    out.pos = vec4<f32>(px.x / view.b.z * 2.0 - 1.0, 1.0 - px.y / view.b.w * 2.0, 0.0, 1.0);
    out.uv = uv;
    return out;
}

@fragment
fn fs_main(in: Out) -> @location(0) vec4<f32> {
    let color = textureSample(canvas_tex, canvas_sampler, fract(in.uv));
    let cell = vec2<i32>(floor(in.pos.xy / view.c.w));
    let checker = select(0.80, 0.92, ((cell.x + cell.y) & 1) == 0);
    let own = all(in.uv >= vec2<f32>(0.0)) && all(in.uv <= vec2<f32>(1.0));
    let dim = select(0.62, 1.0, own);
    return vec4<f32>((color.rgb + vec3<f32>(checker) * (1.0 - color.a)) * dim, 1.0);
}
