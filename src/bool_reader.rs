use crate::Result;
use crate::error::Vp9ParserError;
use bitreader::BitReader;

pub struct BoolReader<'a, 'b> {
    br: &'a mut BitReader<'b>,
    range: u16,
    value: u16,
    max_bits: u32,
}

impl<'a, 'b> BoolReader<'a, 'b> {
    pub fn init(br: &'a mut BitReader<'b>, sz: u32) -> Result<Self> {
        if sz < 1 {
            return Err(Vp9ParserError::InvalidPadding); // Placeholder error: invalid size
        }
        
        // sz is number of bytes to be read
        let value = br.read_u8(8)? as u16;
        let mut reader = BoolReader {
            br,
            range: 255,
            value,
            max_bits: 8 * sz - 8,
        };
        
        let marker = reader.read_bool(128)?;
        if marker {
            return Err(Vp9ParserError::InvalidPadding); // Marker must be 0
        }
        
        Ok(reader)
    }

    pub fn read_bool(&mut self, p: u8) -> Result<bool> {
        let split = 1 + (((self.range - 1) * p as u16) >> 8);
        // eprintln!("[VP9_TRACE] read_bool: p={}, range={}, value={}, split={}, max_bits={}", p, self.range, self.value, split, self.max_bits);
        
        let bit = if self.value < split {
            self.range = split;
            false
        } else {
            self.range -= split;
            self.value -= split;
            true
        };

        while self.range < 128 {
            let new_bit = if self.max_bits > 0 {
                let bit = self.br.read_bool()?;
                self.max_bits -= 1;
                bit
            } else {
                false // Spec requirement says this should never happen.
            };
            self.range <<= 1;
            self.value = (self.value << 1) + (new_bit as u16);
        }

        Ok(bit)
    }

    pub fn exit(&mut self) -> Result<()> {
        let mut remaining = self.max_bits;
        while remaining > 0 {
            let chunk = std::cmp::min(remaining, 64);
            let _ = self.br.read_u64(chunk as u8)?;
            remaining -= chunk;
        }
        Ok(())
    }

    pub fn read_literal(&mut self, n: u8) -> Result<u32> {
        let mut x = 0;
        for _ in 0..n {
            x = 2 * x + (self.read_bool(128)? as u32);
        }
        Ok(x)
    }

    pub fn decode_term_subexp(&mut self) -> Result<u8> {
        if self.read_literal(1)? == 0 {
            return Ok(self.read_literal(4)? as u8);
        }
        if self.read_literal(1)? == 0 {
            return Ok((self.read_literal(4)? + 16) as u8);
        }
        if self.read_literal(1)? == 0 {
            return Ok((self.read_literal(5)? + 32) as u8);
        }
        let v = self.read_literal(7)?;
        if v < 65 {
            return Ok((v + 64) as u8);
        }
        let bit = self.read_literal(1)?;
        Ok(((v << 1) - 1 + bit) as u8)
    }

    pub fn diff_update_prob(&mut self, prob: u8) -> Result<u8> {
        let update_prob = self.read_bool(252)?;
        if update_prob {
            let delta_prob = self.decode_term_subexp()?;
            Ok(inv_remap_prob(delta_prob, prob))
        } else {
            Ok(prob)
        }
    }
}

const INV_MAP_TABLE: &[u8] = &[
    7, 20, 33, 46, 59, 72, 85, 98, 111, 124, 137, 150, 163, 176, 189,
    202, 215, 228, 241, 254, 1, 2, 3, 4, 5, 6, 8, 9, 10, 11,
    12, 13, 14, 15, 16, 17, 18, 19, 21, 22, 23, 24, 25, 26, 27,
    28, 29, 30, 31, 32, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43,
    44, 45, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 60,
    61, 62, 63, 64, 65, 66, 67, 68, 69, 70, 71, 73, 74, 75, 76,
    77, 78, 79, 80, 81, 82, 83, 84, 86, 87, 88, 89, 90, 91, 92,
    93, 94, 95, 96, 97, 99, 100, 101, 102, 103, 104, 105, 106, 107, 108,
    109, 110, 112, 113, 114, 115, 116, 117, 118, 119, 120, 121, 122, 123, 125,
    126, 127, 128, 129, 130, 131, 132, 133, 134, 135, 136, 138, 139, 140, 141,
    142, 143, 144, 145, 146, 147, 148, 149, 151, 152, 153, 154, 155, 156, 157,
    158, 159, 160, 161, 162, 164, 165, 166, 167, 168, 169, 170, 171, 172, 173,
    174, 175, 177, 178, 179, 180, 181, 182, 183, 184, 185, 186, 187, 188, 190,
    191, 192, 193, 194, 195, 196, 197, 198, 199, 200, 201, 203, 204, 205, 206,
    207, 208, 209, 210, 211, 212, 213, 214, 216, 217, 218, 219, 220, 221, 222,
    223, 224, 225, 226, 227, 229, 230, 231, 232, 233, 234, 235, 236, 237, 238,
    239, 240, 242, 243, 244, 245, 246, 247, 248, 249, 250, 251, 252, 253, 253,
];

fn inv_recenter_nonneg(v: i16, m: i16) -> i16 {
    if v > 2 * m {
        v
    } else if v & 1 == 1 {
        m - ((v + 1) >> 1)
    } else {
        m + (v >> 1)
    }
}

fn inv_remap_prob(delta_prob: u8, prob: u8) -> u8 {
    let v = INV_MAP_TABLE[delta_prob as usize] as i16;
    let p = prob as i16;
    let m = if (p << 1) <= 255 {
        inv_recenter_nonneg(v, p) + 1
    } else {
        255 - inv_recenter_nonneg(v, 255 - 1 - p)
    };
    m as u8
}
