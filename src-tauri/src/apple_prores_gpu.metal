#include <metal_stdlib>
using namespace metal;

struct Params {
    uint width;
    uint height;
    uint transfer; // 0 = SDR, 1 = PQ, 2 = HLG
    uint alpha;
};

static float3 encode_rgb(float3 light, uint transfer) {
    // Keep the linear BT.2020 input (1.0 = 100 cd/m²) and transfer math in
    // sync with mobile_hdr::Transfer.
    if (transfer == 1) {
        float3 x = pow(clamp(light / 100.0f, 0.0f, 1.0f), 0.15930176f);
        return pow((0.8359375f + 18.851563f * x) / (1.0f + 18.6875f * x), 78.84375f);
    }
    if (transfer == 2) {
        float3 linear = clamp(light / 10.0f, 0.0f, 1.0f);
        float luminance = dot(linear, float3(0.2627f, 0.678f, 0.0593f));
        // Invert the 1,000-nit reference HLG display OOTF before the OETF.
        float scale = luminance > 0.0f ? pow(luminance, -1.0f / 6.0f) : 0.0f;
        float3 x = min(linear * scale, 1.0f);
        // Avoid log of negative values in lanes selected from the sqrt branch.
        float3 upper = 0.17883277f * log(12.0f * max(x, 1.0f / 12.0f) - 0.28466892f) + 0.5599107f;
        return select(upper, sqrt(3.0f * x), x <= 1.0f / 12.0f);
    }
    return light;
}

static float3 to_ycbcr(float3 rgb, uint transfer) {
    float kr = transfer == 0 ? 0.2126f : 0.2627f;
    float kb = transfer == 0 ? 0.0722f : 0.0593f;
    rgb = encode_rgb(rgb, transfer);
    float y = dot(rgb, float3(kr, 1.0f - kr - kb, kb));
    return float3(y, (rgb.b - y) / (2.0f * (1.0f - kb)), (rgb.r - y) / (2.0f * (1.0f - kr)));
}

kernel void prores_x422(texture2d<float, access::read> source [[texture(0)]],
                        device ushort2 *output [[buffer(0)]],
                        constant Params &params [[buffer(1)]],
                        uint2 pos [[thread_position_in_grid]]) {
    uint pairs = params.width / 2;
    if (pos.x >= pairs || pos.y >= params.height) {
        return;
    }
    // The offscreen texture is bottom-first. Match the BGRA enqueue flip.
    uint2 src = uint2(pos.x * 2, params.height - 1 - pos.y);
    float3 first = to_ycbcr(source.read(src).rgb, params.transfer);
    float3 second = to_ycbcr(source.read(src + uint2(1, 0)).rgb, params.transfer);
    float2 y = clamp(64.0f + 876.0f * float2(first.x, second.x), 64.0f, 940.0f);
    float2 cbcr = clamp(512.0f + 448.0f * (first.yz + second.yz), 64.0f, 960.0f);
    uint index = pos.y * pairs + pos.x;
    // Video-range 10-bit samples use the high bits of each little-endian word.
    output[index] = ushort2(round(y)) << 6;
    output[index + pairs * params.height] = ushort2(round(cbcr)) << 6;
}

kernel void prores_y416(texture2d<float, access::read> source [[texture(0)]],
                        device ushort4 *output [[buffer(0)]],
                        constant Params &params [[buffer(1)]],
                        uint2 pos [[thread_position_in_grid]]) {
    if (pos.x >= params.width || pos.y >= params.height) {
        return;
    }
    float4 rgba = source.read(uint2(pos.x, params.height - 1 - pos.y));
    // Renderer alpha exports contain associated (premultiplied) RGB. y416 / VT
    // consumes unassociated color: divide BEFORE transfer encoding and YCbCr.
    // At zero coverage the hidden color is defined as black, never NaN/Inf.
    // Opaque profiles intentionally retain their existing RGB behavior.
    float alpha = clamp(rgba.a, 0.0f, 1.0f);
    float3 rgb = params.alpha != 0
        ? (alpha > 0.0f ? rgba.rgb / alpha : float3(0.0f))
        : rgba.rgb;
    float3 ycbcr = to_ycbcr(rgb, params.transfer);
    // CoreVideo's y416 has full-range A first, then video-range Y', Cb, Cr.
    float a = params.alpha != 0 ? alpha * 65535.0f : 65535.0f;
    float y = clamp(4096.0f + 56064.0f * ycbcr.x, 4096.0f, 60160.0f);
    float2 cbcr = clamp(32768.0f + 57344.0f * ycbcr.yz, 4096.0f, 61440.0f);
    output[pos.y * params.width + pos.x] = ushort4(round(float4(a, y, cbcr.x, cbcr.y)));
}
