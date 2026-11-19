#[derive(Clone, Copy, Debug)]
pub(crate) enum Transfer {
    Pq,
    Hlg,
}

impl Transfer {
    pub(crate) fn from_config(config: &crate::render::RenderConfig) -> Option<Self> {
        config.is_hdr().then(|| {
            if config.color_space.eq_ignore_ascii_case("rec2100_pq")
                || config.color_space.eq_ignore_ascii_case("rec2100-pq")
            {
                Self::Pq
            } else {
                Self::Hlg
            }
        })
    }

    #[cfg(any(target_os = "ios", feature = "apple-render-check"))]
    pub(crate) fn encode_rgb(self, light: [f32; 3]) -> [f32; 3] {
        match self {
            Self::Pq => light.map(|value| self.encode(value)),
            Self::Hlg => {
                let values = light.map(|value| (value / 10.0).clamp(0.0, 1.0));
                let luminance = 0.2627 * values[0] + 0.678 * values[1] + 0.0593 * values[2];
                let scale = if luminance > 0.0 {
                    luminance.powf(-1.0 / 6.0)
                } else {
                    0.0
                };
                values.map(|value| self.encode((value * scale).min(1.0)))
            }
        }
    }

    // The scene is linear BT.2020, with 1.0 equal to 100 cd/m².
    // Cache the transfer for each possible half-float input sample.
    fn encode(self, light: f32) -> f32 {
        let light = light.max(0.0);
        match self {
            Self::Pq => {
                let x = (light / 100.0).min(1.0).powf(0.159_301_76);
                ((0.835_937_5 + 18.851_563 * x) / (1.0 + 18.687_5 * x)).powf(78.843_75)
            }
            Self::Hlg => {
                // Invert the 1,000-nit reference HLG display OOTF at each
                // pixel before applying the HLG OETF to the three channels.
                if light <= 1.0 / 12.0 {
                    (3.0 * light).sqrt()
                } else {
                    0.178_832_77 * (12.0 * light - 0.284_668_92).ln() + 0.559_910_7
                }
            }
        }
    }
}

pub(crate) struct P010 {
    pub(crate) rgba: Vec<u8>,
    pq: Option<Vec<f32>>,
    width: usize,
    height: usize,
    transfer: Transfer,
}

impl P010 {
    pub(crate) fn new(width: u32, height: u32, transfer: Transfer) -> Self {
        let width = width as usize;
        let height = height as usize;
        Self {
            rgba: vec![0; width * height * 8],
            pq: matches!(transfer, Transfer::Pq).then(|| {
                (0..=u16::MAX)
                    .map(|bits| transfer.encode(half::f16::from_bits(bits).to_f32()))
                    .collect()
            }),
            width,
            height,
            transfer,
        }
    }

    pub(crate) fn convert(&self, output: &mut [u8]) {
        const KR: f32 = 0.2627;
        const KB: f32 = 0.0593;
        const KG: f32 = 1.0 - KR - KB;
        fn store(plane: &mut [u8], index: usize, code: f32) {
            plane[index * 2..index * 2 + 2]
                .copy_from_slice(&((code.round() as u16) << 6).to_le_bytes());
        }
        let (y_plane, uv_plane) = output.split_at_mut(self.width * self.height * 2);
        for row in (0..self.height).step_by(2) {
            for col in (0..self.width).step_by(2) {
                let mut cb = 0.0;
                let mut cr = 0.0;
                for dy in 0..2 {
                    for dx in 0..2 {
                        let index = (row + dy) * self.width + col + dx;
                        let bits = [0, 2, 4].map(|offset| {
                            let start = index * 8 + offset;
                            u16::from_ne_bytes([self.rgba[start], self.rgba[start + 1]])
                        });
                        let [r, g, b] = if let Some(pq) = &self.pq {
                            bits.map(|bits| pq[bits as usize])
                        } else {
                            let values = bits.map(|bits| {
                                (half::f16::from_bits(bits).to_f32() / 10.0).clamp(0.0, 1.0)
                            });
                            let luminance =
                                (KR * values[0] + KG * values[1] + KB * values[2]).max(0.0);
                            let scale = if luminance > 0.0 {
                                luminance.powf(-1.0 / 6.0)
                            } else {
                                0.0
                            };
                            values.map(|value| self.transfer.encode((value * scale).min(1.0)))
                        };
                        let y = KR * r + KG * g + KB * b;
                        store(y_plane, index, (64.0 + 876.0 * y).clamp(64.0, 940.0));
                        cb += (b - y) / (2.0 * (1.0 - KB));
                        cr += (r - y) / (2.0 * (1.0 - KR));
                    }
                }
                let index = (row / 2) * self.width + col;
                store(uv_plane, index, (512.0 + 224.0 * cb).clamp(64.0, 960.0));
                store(uv_plane, index + 1, (512.0 + 224.0 * cr).clamp(64.0, 960.0));
            }
        }
    }
}

/// Verify the delivered HEVC sequence parameter set uses 10-bit 4:2:0.
pub(crate) fn is_10_bit_420_sps(nal: &[u8]) -> Option<bool> {
    if nal.len() < 3 || (nal[0] >> 1) & 63 != 33 {
        return None;
    }
    let mut rbsp = Vec::with_capacity(nal.len() - 2);
    let mut zeros = 0;
    for &byte in &nal[2..] {
        if zeros == 2 && byte == 3 {
            zeros = 0;
            continue;
        }
        zeros = if byte == 0 { zeros + 1 } else { 0 };
        rbsp.push(byte);
    }

    let mut position = 0;
    let mut read = |count: usize| -> Option<u32> {
        let end = position + count;
        if end > rbsp.len() * 8 {
            return None;
        }
        let mut value = 0u32;
        for bit in position..end {
            value = (value << 1) | u32::from((rbsp[bit / 8] >> (7 - bit % 8)) & 1);
        }
        position = end;
        Some(value)
    };
    read(4)?;
    let sub_layers = read(3)? as usize;
    read(1)?;
    read(96)?; // general profile_tier_level
    let mut profiles = [0; 7];
    for flag in &mut profiles[..sub_layers] {
        *flag = read(2)?;
    }
    if sub_layers > 0 {
        read((8 - sub_layers) * 2)?; // reserved_zero_2bits
    }
    for flag in &profiles[..sub_layers] {
        if flag & 2 != 0 {
            read(88)?;
        }
        if flag & 1 != 0 {
            read(8)?;
        }
    }

    read_ue(&mut read)?; // sps_seq_parameter_set_id
    let chroma_format = read_ue(&mut read)?;
    if chroma_format != 1 {
        return Some(false);
    }
    read_ue(&mut read)?; // pic_width_in_luma_samples
    read_ue(&mut read)?; // pic_height_in_luma_samples
                         // The conformance flag is one bit, unlike the neighbouring fields.
    if read(1)? != 0 {
        for _ in 0..4 {
            read_ue(&mut read)?;
        }
    }
    Some(read_ue(&mut read)? == 2 && read_ue(&mut read)? == 2)
}

fn read_ue(read: &mut impl FnMut(usize) -> Option<u32>) -> Option<u32> {
    let mut zeros = 0;
    while read(1)? == 0 {
        zeros += 1;
        if zeros > 31 {
            return None;
        }
    }
    Some((1u32 << zeros) - 1 + read(zeros)?)
}
