use rand::{thread_rng, Rng};
use vp9_parser::{FrameType, Profile, Vp9Parser};

struct BitWriter {
    data: Vec<u8>,
    current_byte: u8,
    bit_count: u8,
}

impl BitWriter {
    fn new() -> Self {
        BitWriter {
            data: Vec::new(),
            current_byte: 0,
            bit_count: 0,
        }
    }

    fn write_bit(&mut self, b: bool) {
        if b {
            self.current_byte |= 1 << (7 - self.bit_count);
        }
        self.bit_count += 1;
        if self.bit_count == 8 {
            self.data.push(self.current_byte);
            self.current_byte = 0;
            self.bit_count = 0;
        }
    }

    fn write_bits(&mut self, value: u64, count: u8) {
        for i in (0..count).rev() {
            let bit = ((value >> i) & 1) == 1;
            self.write_bit(bit);
        }
    }

    fn write_inverse_i8(&mut self, value: i8, bits: u8) {
        let magnitude = value.unsigned_abs();
        self.write_bits(magnitude as u64, bits);
        let is_negative = value < 0;
        self.write_bit(is_negative);
    }

    fn write_inverse_i16(&mut self, value: i16, bits: u8) {
        let magnitude = value.unsigned_abs();
        self.write_bits(magnitude as u64, bits);
        let is_negative = value < 0;
        self.write_bit(is_negative);
    }

    fn finish(mut self) -> Vec<u8> {
        if self.bit_count > 0 {
            self.data.push(self.current_byte);
        }
        self.data
    }
}

#[derive(Debug, Clone)]
struct FuzzerState {
    ref_frame_sizes: [(u16, u16); 8],
    ref_frame_indices: [u8; 3],
}

impl FuzzerState {
    fn new() -> Self {
        FuzzerState {
            ref_frame_sizes: [(0, 0); 8],
            ref_frame_indices: [0; 3],
        }
    }
}

struct ExpectedFrame {
    profile: Profile,
    frame_type: FrameType,
    show_frame: bool,
    error_resilient_mode: bool,
    width: u16,
    height: u16,
    render_width: u16,
    render_height: u16,
    refresh_frame_flags: u8,
    ref_frame_indices: [u8; 3],
    tile_cols_log2: u8,
    tile_rows_log2: u8,
}

fn write_color_config<R: Rng>(rng: &mut R, bw: &mut BitWriter, profile: Profile) {
    if profile >= Profile::Profile2 {
        bw.write_bit(rng.gen_bool(0.5));
    }
    let cs_val = rng.gen_range(1..8);
    bw.write_bits(cs_val as u64, 3);
    if cs_val != 7 {
        // Not RGB
        bw.write_bit(rng.gen_bool(0.5)); // color_range
        if profile == Profile::Profile1 || profile == Profile::Profile3 {
            bw.write_bit(rng.gen_bool(0.5)); // subsampling_x
            bw.write_bit(rng.gen_bool(0.5)); // subsampling_y
            bw.write_bit(false); // reserved
        }
    } else if profile == Profile::Profile1 || profile == Profile::Profile3 {
        bw.write_bit(false); // reserved
    }
}

fn write_frame_size<R: Rng>(rng: &mut R, bw: &mut BitWriter) -> (u16, u16) {
    let w = rng.gen_range(16..1024);
    let h = rng.gen_range(16..1024);
    bw.write_bits((w - 1) as u64, 16);
    bw.write_bits((h - 1) as u64, 16);
    (w, h)
}

fn write_render_size<R: Rng>(rng: &mut R, bw: &mut BitWriter, w: u16, h: u16) -> (u16, u16) {
    let diff = rng.gen_bool(0.2);
    bw.write_bit(diff);
    if diff {
        let rw = rng.gen_range(16..1024);
        let rh = rng.gen_range(16..1024);
        bw.write_bits((rw - 1) as u64, 16);
        bw.write_bits((rh - 1) as u64, 16);
        (rw, rh)
    } else {
        (w, h)
    }
}

fn write_interpolation_filter<R: Rng>(rng: &mut R, bw: &mut BitWriter) {
    let switchable = rng.gen_bool(0.5);
    bw.write_bit(switchable);
    if !switchable {
        bw.write_bits(rng.gen_range(0..4), 2);
    }
}

fn write_uncompressed_header<R: Rng>(
    rng: &mut R,
    bw: &mut BitWriter,
    state: &mut FuzzerState,
) -> ExpectedFrame {
    // 1. Frame Marker (must be 2)
    bw.write_bits(2, 2);

    // 2. Profile
    let profile_val = rng.gen_range(0..=3);
    let profile_low = profile_val & 1;
    let profile_high = (profile_val >> 1) & 1;
    bw.write_bit(profile_low == 1);
    bw.write_bit(profile_high == 1);
    let profile = match profile_val {
        0 => Profile::Profile0,
        1 => Profile::Profile1,
        2 => Profile::Profile2,
        3 => Profile::Profile3,
        _ => unreachable!(),
    };
    if profile == Profile::Profile3 {
        bw.write_bit(false);
    }

    // 3. show_existing_frame
    bw.write_bit(false);

    // 4. frame_type
    let has_valid_refs = state.ref_frame_sizes.iter().any(|s| s.0 > 0 && s.1 > 0);
    let frame_type = if has_valid_refs && rng.gen_bool(0.7) {
        FrameType::NonKeyFrame
    } else {
        FrameType::KeyFrame
    };
    bw.write_bit(frame_type == FrameType::NonKeyFrame);

    // 5. show_frame
    let show_frame = rng.gen_bool(0.8);
    bw.write_bit(show_frame);

    // 6. error_resilient_mode
    let error_resilient_mode = rng.gen_bool(0.2);
    bw.write_bit(error_resilient_mode);

    let mut refresh_frame_flags = 0xFF; // Default for KeyFrame

    let mut width = 0;
    let mut height = 0;
    let render_width;
    let render_height;
    let mut ref_frame_indices = [0u8; 3];

    if frame_type == FrameType::KeyFrame {
        // Sync code
        bw.write_bits(0x49, 8);
        bw.write_bits(0x83, 8);
        bw.write_bits(0x42, 8);

        // Color config
        write_color_config(rng, bw, profile);

        // Frame Size
        let (w, h) = write_frame_size(rng, bw);
        width = w;
        height = h;

        // Render Size
        let (rw, rh) = write_render_size(rng, bw, w, h);
        render_width = rw;
        render_height = rh;
    } else {
        let mut intra_only_val = false;
        if !show_frame {
            intra_only_val = rng.gen_bool(0.3);
            bw.write_bit(intra_only_val);
        }

        if !error_resilient_mode {
            bw.write_bits(rng.gen_range(0..4), 2); // reset_frame_context
        }

        if intra_only_val {
            // Sync code
            bw.write_bits(0x49, 8);
            bw.write_bits(0x83, 8);
            bw.write_bits(0x42, 8);

            if profile > Profile::Profile0 {
                write_color_config(rng, bw, profile);
            }
            refresh_frame_flags = rng.gen_range(0..=255);
            bw.write_bits(refresh_frame_flags as u64, 8);
            let (w, h) = write_frame_size(rng, bw);
            width = w;
            height = h;
            let (rw, rh) = write_render_size(rng, bw, w, h);
            render_width = rw;
            render_height = rh;
        } else {
            refresh_frame_flags = rng.gen_range(0..=255);
            bw.write_bits(refresh_frame_flags as u64, 8);

            for idx_out in &mut ref_frame_indices {
                let idx = rng.gen_range(0..8);
                bw.write_bits(idx as u64, 3);
                *idx_out = idx as u8;
                bw.write_bit(rng.gen_bool(0.5)); // sign_bias
            }

            let mut found_ref = false;
            for idx in ref_frame_indices {
                let check_ref = rng.gen_bool(0.3);
                bw.write_bit(check_ref);
                if check_ref {
                    let sizes = state.ref_frame_sizes[idx as usize];
                    width = sizes.0;
                    height = sizes.1;
                    found_ref = true;
                    break;
                }
            }
            if !found_ref {
                let (w, h) = write_frame_size(rng, bw);
                width = w;
                height = h;
            }

            let (rw, rh) = write_render_size(rng, bw, width, height);
            render_width = rw;
            render_height = rh;

            bw.write_bit(rng.gen_bool(0.5)); // allow_high_precision_mv
            write_interpolation_filter(rng, bw);
        }
    }

    if !error_resilient_mode {
        bw.write_bit(rng.gen_bool(0.5)); // refresh_frame_context
        bw.write_bit(rng.gen_bool(0.5)); // frame_parallel_decoding_mode
    }
    bw.write_bits(rng.gen_range(0..4), 2); // frame_context_idx

    // Loop Filter Params
    let loop_filter_level = rng.gen_range(0..64);
    bw.write_bits(loop_filter_level as u64, 6);
    let loop_filter_sharpness = rng.gen_range(0..8);
    bw.write_bits(loop_filter_sharpness as u64, 3);
    let loop_filter_delta_enabled = rng.gen_bool(0.5);
    bw.write_bit(loop_filter_delta_enabled);

    if loop_filter_delta_enabled {
        let delta_update = rng.gen_bool(0.5);
        bw.write_bit(delta_update);
        if delta_update {
            for _ in 0..4 {
                // ref deltas
                let update = rng.gen_bool(0.5);
                bw.write_bit(update);
                if update {
                    bw.write_inverse_i8(rng.gen_range(-63..64), 6);
                }
            }
            for _ in 0..2 {
                // mode deltas
                let update = rng.gen_bool(0.5);
                bw.write_bit(update);
                if update {
                    bw.write_inverse_i8(rng.gen_range(-63..64), 6);
                }
            }
        }
    }

    // Quantization Params
    let base_q_idx = rng.gen_range(0..=255);
    bw.write_bits(base_q_idx as u64, 8);

    let write_delta_q = |rng: &mut R, bw: &mut BitWriter| {
        let coded = rng.gen_bool(0.5);
        bw.write_bit(coded);
        if coded {
            bw.write_inverse_i8(rng.gen_range(-7..8), 4);
        }
    };

    write_delta_q(rng, bw); // y_dc
    write_delta_q(rng, bw); // uv_dc
    write_delta_q(rng, bw); // uv_ac

    // Segmentation Params
    let segmentation_enabled = rng.gen_bool(0.2);
    bw.write_bit(segmentation_enabled);
    if segmentation_enabled {
        let update_map = rng.gen_bool(0.5);
        bw.write_bit(update_map);
        if update_map {
            for _ in 0..7 {
                // tree probs
                let coded = rng.gen_bool(0.5);
                bw.write_bit(coded);
                if coded {
                    bw.write_bits(rng.gen_range(0..=255), 8);
                }
            }
            let temporal_update = rng.gen_bool(0.5);
            bw.write_bit(temporal_update);
            for _ in 0..3 {
                // pred probs
                if temporal_update {
                    let coded = rng.gen_bool(0.5);
                    bw.write_bit(coded);
                    if coded {
                        bw.write_bits(rng.gen_range(0..=255), 8);
                    }
                }
            }
        }
        let update_data = rng.gen_bool(0.5);
        bw.write_bit(update_data);
        if update_data {
            bw.write_bit(rng.gen_bool(0.5)); // abs or delta
            for _ in 0..8 {
                // max segments
                // alt_q (8 bits, signed)
                let enabled = rng.gen_bool(0.5);
                bw.write_bit(enabled);
                if enabled {
                    bw.write_inverse_i16(rng.gen_range(-255..256), 8);
                }

                // alt_l (6 bits, signed)
                let enabled = rng.gen_bool(0.5);
                bw.write_bit(enabled);
                if enabled {
                    bw.write_inverse_i16(rng.gen_range(-63..64), 6);
                }

                // ref_frame (2 bits, unsigned)
                let enabled = rng.gen_bool(0.5);
                bw.write_bit(enabled);
                if enabled {
                    bw.write_bits(rng.gen_range(0..4), 2);
                }

                // skip (0 bits, unsigned)
                bw.write_bit(rng.gen_bool(0.5));
            }
        }
    }

    // Tile Info
    let mi_cols = (width + 7) >> 3;
    let sb64_cols = (mi_cols + 7) >> 3;

    let mut min_log2 = 0;
    while (64 << min_log2) < sb64_cols {
        min_log2 += 1;
    }
    let mut max_log2 = 1;
    while (sb64_cols >> max_log2) >= 4 {
        max_log2 += 1;
    }
    let mut max_log2_final = max_log2 - 1;
    if max_log2_final < min_log2 {
        max_log2_final = min_log2;
    }
    if max_log2_final > 6 {
        max_log2_final = 6;
    }

    let mut tile_cols = min_log2;
    while tile_cols < max_log2_final {
        let inc = rng.gen_bool(0.5);
        bw.write_bit(inc);
        if inc {
            tile_cols += 1;
        } else {
            break;
        }
    }

    let tile_rows = rng.gen_range(0..2);
    bw.write_bit(tile_rows == 1);
    let mut tile_rows_final = tile_rows;
    if tile_rows == 1 {
        let inc = rng.gen_bool(0.5);
        bw.write_bit(inc);
        if inc {
            tile_rows_final += 1;
        }
    }

    // Compressed Header Size (write 0)
    bw.write_bits(0, 16);

    // Trailing Bits (Align to byte)
    while bw.bit_count > 0 {
        bw.write_bit(false);
    }

    ExpectedFrame {
        profile,
        frame_type,
        show_frame,
        error_resilient_mode,
        width,
        height,
        render_width,
        render_height,
        refresh_frame_flags,
        ref_frame_indices,
        tile_cols_log2: tile_cols,
        tile_rows_log2: tile_rows_final,
    }
}

#[test]
fn test_fuzz_uncompressed_header() {
    let mut rng = thread_rng();
    for _ in 0..100 {
        let mut state = FuzzerState::new();
        let mut parser = Vp9Parser::default();

        for _ in 0..10 {
            let mut bw = BitWriter::new();
            let expected = write_uncompressed_header(&mut rng, &mut bw, &mut state);
            let bytes = bw.finish();

            match parser.parse_packet(bytes.clone()) {
                Ok(frames) => {
                    assert_eq!(frames.len(), 1);
                    let parsed = &frames[0];

                    assert_eq!(parsed.profile(), expected.profile);
                    assert_eq!(parsed.frame_type(), expected.frame_type);
                    assert_eq!(parsed.show_frame(), expected.show_frame);
                    assert_eq!(parsed.error_resilient_mode(), expected.error_resilient_mode);
                    assert_eq!(parsed.width(), expected.width);
                    assert_eq!(parsed.height(), expected.height);
                    assert_eq!(parsed.render_width(), expected.render_width);
                    assert_eq!(parsed.render_height(), expected.render_height);
                    assert_eq!(parsed.tile_cols_log2(), expected.tile_cols_log2);
                    assert_eq!(parsed.tile_rows_log2(), expected.tile_rows_log2);

                    let flags = expected.refresh_frame_flags;
                    for j in 0..8 {
                        if (flags >> j) & 1 == 1 {
                            state.ref_frame_sizes[j] = (expected.width, expected.height);
                        }
                    }
                    state.ref_frame_indices = expected.ref_frame_indices;
                }
                Err(e) => {
                    panic!("Failed to parse: {:?}", e);
                }
            }
        }
    }
}
