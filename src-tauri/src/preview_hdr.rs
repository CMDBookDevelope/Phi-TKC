use anyhow::Result;
use macroquad::{
    miniquad::{PresentationColorSpace, UniformType},
    prelude::*,
    window::miniquad_context,
};
use prpr::core::MSRenderTarget;

pub(super) struct Frame {
    size: (u32, u32),
    scene: MSRenderTarget,
    overlay: MSRenderTarget,
}

impl Frame {
    pub(super) fn new(size: (u32, u32)) -> Self {
        Self {
            size,
            scene: MSRenderTarget::new_hdr(size, 1),
            overlay: MSRenderTarget::new_hdr(size, 1),
        }
    }

    pub(super) fn size(&self) -> (u32, u32) {
        self.size
    }

    pub(super) fn scene(&self) -> RenderTarget {
        self.scene.input()
    }

    pub(super) fn overlay(&self) -> RenderTarget {
        self.overlay.input()
    }

    pub(super) fn present(&self, material: Material, color_space: PresentationColorSpace) {
        let transfer = match color_space {
            PresentationColorSpace::Srgb => 0_i32,
            PresentationColorSpace::LinearSrgb => 1,
            PresentationColorSpace::Rec2100Pq => 2,
            PresentationColorSpace::Rec2100Hlg => 3,
            PresentationColorSpace::ExtendedLinearSrgb => 4,
            PresentationColorSpace::AppleEdr => 5,
        };
        set_default_camera();
        material.set_uniform("transfer", transfer);
        material.set_texture("Overlay", self.overlay().texture);
        gl_use_material(material);
        draw_texture_ex(
            self.scene().texture,
            0.0,
            0.0,
            WHITE,
            DrawTextureParams {
                dest_size: Some(vec2(self.size.0 as f32, self.size.1 as f32)),
                ..Default::default()
            },
        );
        gl_use_default_material();
    }
}

pub(super) fn color_space() -> PresentationColorSpace {
    let ctx = miniquad_context();
    if let Some(vulkan) = ctx.vulkan_context() {
        return vulkan.presentation_color_space();
    }
    #[cfg(target_os = "macos")]
    if let Some(metal) = ctx.metal_context() {
        return if metal.window_edr_enabled() {
            PresentationColorSpace::AppleEdr
        } else {
            PresentationColorSpace::LinearSrgb
        };
    }
    PresentationColorSpace::Srgb
}

pub(super) fn load() -> Result<Material> {
    Ok(load_material(
        VERTEX,
        FRAGMENT,
        MaterialParams {
            uniforms: vec![("transfer".into(), UniformType::Int1)],
            textures: vec!["Overlay".into()],
            ..Default::default()
        },
    )?)
}

const VERTEX: &str = r#"#version 450 core
layout(location = 0) in vec3 position;
layout(location = 1) in vec2 texcoord;
layout(set = 0, binding = 0) uniform Uniforms { mat4 Projection; mat4 Model; vec4 _Time; int transfer; };
layout(location = 0) out vec2 uv;
void main() {
    gl_Position = Projection * Model * vec4(position, 1.0);
    uv = texcoord;
}
"#;

const FRAGMENT: &str = r#"#version 450 core
layout(location = 0) in vec2 uv;
layout(set = 0, binding = 0) uniform Uniforms { mat4 Projection; mat4 Model; vec4 _Time; int transfer; };
layout(set = 0, binding = 1) uniform sampler2D Texture;
layout(set = 0, binding = 3) uniform sampler2D Overlay;
layout(location = 0) out vec4 fragColor;

const mat3 SRGB_TO_BT2020 = mat3(
    0.627404, 0.069097, 0.016391,
    0.329283, 0.919540, 0.088013,
    0.043313, 0.011362, 0.895595
);
const mat3 BT2020_TO_SRGB = mat3(
     1.660491, -0.124550, -0.018151,
    -0.587641,  1.132900, -0.100579,
    -0.072850, -0.008349,  1.118730
);

vec3 srgbToLinear(vec3 v) {
    return mix(pow((v + 0.055) / 1.055, vec3(2.4)), v / 12.92, lessThanEqual(v, vec3(0.04045)));
}

vec3 linearToSrgb(vec3 v) {
    return mix(1.055 * pow(v, vec3(1.0 / 2.4)) - 0.055, 12.92 * v, lessThanEqual(v, vec3(0.0031308)));
}

vec3 pq(vec3 rgb) {
    // prpr normalizes 100 cd/m² to 1.0. ST 2084 uses 10,000 cd/m².
    vec3 p = pow(clamp(rgb * 0.01, 0.0, 1.0), vec3(2610.0 / 16384.0));
    return pow((3424.0 / 4096.0 + 2413.0 / 128.0 * p) / (1.0 + 2392.0 / 128.0 * p), vec3(2523.0 / 32.0));
}

vec3 hlg(vec3 rgb) {
    // BT.2100 inverse OOTF for the 1000-nit reference display, then the HLG OETF.
    vec3 displayLinear = max(rgb * 0.1, 0.0);
    float yd = dot(displayLinear, vec3(0.2627, 0.6780, 0.0593));
    vec3 sceneLinear = clamp(displayLinear * pow(max(yd, 0.000001), 1.0 / 1.2 - 1.0), 0.0, 1.0);
    const float a = 0.17883277;
    const float b = 0.28466892;
    const float c = 0.55991073;
    return mix(a * log(max(12.0 * sceneLinear - b, 0.000001)) + c,
               sqrt(3.0 * sceneLinear), lessThanEqual(sceneLinear, vec3(1.0 / 12.0)));
}

void main() {
    vec3 scene = texture(Texture, uv).rgb;
    vec4 overlay = texture(Overlay, uv);
    // The overlay is drawn into a transparent SDR target using normal alpha blending.
    vec3 overlayLinear = srgbToLinear(clamp(overlay.rgb / max(overlay.a, 0.000001), 0.0, 1.0));
    vec3 rgb = mix(scene, SRGB_TO_BT2020 * overlayLinear, overlay.a);
    vec3 encoded;
    if (transfer == 2) {
        encoded = pq(rgb);
    } else if (transfer == 3) {
        encoded = hlg(rgb);
    } else if (transfer == 4) {
        // scRGB uses Rec.709 primaries and 80 cd/m² per unit.
        encoded = BT2020_TO_SRGB * rgb * (100.0 / 80.0);
    } else if (transfer == 5) {
        // Core Animation EDR interprets 1.0 as the display white point.
        encoded = BT2020_TO_SRGB * rgb;
    } else {
        // Preserve SDR colors and fit values above reference white into the SDR range.
        vec3 sdr = max(BT2020_TO_SRGB * scene, 0.0);
        float peak = max(max(sdr.r, sdr.g), sdr.b);
        float shoulder = peak <= 0.75 ? peak : 1.0 - 0.25 * exp(-4.0 * (peak - 0.75));
        sdr *= shoulder / max(peak, 0.000001);
        sdr = mix(sdr, overlayLinear, overlay.a);
        encoded = transfer == 1 ? sdr : linearToSrgb(sdr);
    }
    fragColor = vec4(encoded, 1.0);
}
"#;
