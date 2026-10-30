// Nodes, frames and areas. Positions are relative to the scene origin; sizes are in physical
// pixels and stay constant at any zoom.

struct Uniforms {
    view_proj: mat4x4<f32>,
    // Viewport size in physical pixels.
    viewport: vec2<f32>,
    // x: 1 when the target applies the sRGB curve itself.
    flags: vec2<f32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;

struct Out {
    @builtin(position) clip: vec4<f32>,
    @location(0) colour: vec4<f32>,
};

// The corners of a quad as two triangles: (-1,-1) (1,-1) (1,1) (-1,-1) (1,1) (-1,1).
fn corner(i: u32) -> vec2<f32> {
    var c = array<vec2<f32>, 6>(vec2(-1.0, -1.0), vec2(1.0, -1.0), vec2(1.0, 1.0), vec2(-1.0, -1.0), vec2(1.0, 1.0), vec2(-1.0, 1.0));
    return c[i];
}

// sRGB → linear, since the target applies the sRGB curve on write.
fn linear(c: vec4<f32>) -> vec4<f32> {
    let rgb = select(pow((c.rgb + 0.055) / 1.055, vec3(2.4)), c.rgb / 12.92, c.rgb <= vec3(0.04045));
    return vec4(rgb, c.a);
}

@vertex
fn vs_point(@builtin(vertex_index) vi: u32, @location(0) pos: vec3<f32>, @location(1) colour: vec4<f32>, @location(2) size: f32) -> Out {
    var o: Out;
    let clip = u.view_proj * vec4(pos, 1.0);
    o.clip = clip + vec4(corner(vi) * size / u.viewport * clip.w, 0.0, 0.0);
    o.colour = colour;
    return o;
}

@vertex
fn vs_line(
    @builtin(vertex_index) vi: u32,
    @location(0) a: vec3<f32>,
    @location(1) b: vec3<f32>,
    @location(2) colour: vec4<f32>,
    @location(3) width: f32,
) -> Out {
    var o: Out;
    var ca = u.view_proj * vec4(a, 1.0);
    var cb = u.view_proj * vec4(b, 1.0);
    // Clip the segment to the near side so a frame through the eye still draws.
    let near = 1e-5;
    if ca.w < near && cb.w < near {
        o.clip = vec4(0.0, 0.0, -2.0, 1.0);
        o.colour = vec4(0.0);
        return o;
    }
    if ca.w < near {
        ca = mix(ca, cb, (near - ca.w) / (cb.w - ca.w));
    } else if cb.w < near {
        cb = mix(cb, ca, (near - cb.w) / (ca.w - cb.w));
    }
    let sa = ca.xy / ca.w * u.viewport;
    let sb = cb.xy / cb.w * u.viewport;
    var dir = sb - sa;
    if dot(dir, dir) < 1e-12 {
        dir = vec2(1.0, 0.0);
    }
    let normal = normalize(vec2(-dir.y, dir.x));
    let c = corner(vi);
    // x picks the end, y the side.
    let clip = select(ca, cb, c.x > 0.0);
    o.clip = clip + vec4(normal * c.y * width / u.viewport * clip.w, 0.0, 0.0);
    o.colour = colour;
    return o;
}

@vertex
fn vs_area(@location(0) pos: vec3<f32>, @location(1) colour: vec4<f32>) -> Out {
    var o: Out;
    o.clip = u.view_proj * vec4(pos, 1.0);
    o.colour = colour;
    return o;
}

@fragment
fn fs_main(i: Out) -> @location(0) vec4<f32> {
    let c = select(i.colour, linear(i.colour), u.flags.x > 0.5);
    // Premultiplied, as egui blends.
    return vec4(c.rgb * c.a, c.a);
}
