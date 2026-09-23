use super::header::c_atof;
use crate::Error;

/// Payload layout from the `#:` code, decoded as `decodeDataFormat` does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Format {
    /// `L`: every record starts on an 80-byte boundary. Otherwise records are packed (`K`).
    pub long: bool,
    /// `D`: double precision. Otherwise single.
    pub double: bool,
    /// `F`: formatted ASCII. Otherwise big-endian IEEE binary.
    pub ascii: bool,
}

impl Format {
    /// Reads the flags from the first four characters after `#:`.
    pub fn from_code(code: &str) -> Self {
        let code: String = code.chars().take(4).collect();
        Self {
            long: code.contains('L'),
            double: code.contains('D'),
            ascii: code.contains('F'),
        }
    }

    fn value_width(self) -> usize {
        match (self.ascii, self.double) {
            (true, true) => 20,
            (true, false) => 10,
            (false, true) => 8,
            (false, false) => 4,
        }
    }

    /// Bytes per record for `channels` values; `None` on overflow.
    fn record_size(self, channels: usize) -> Option<usize> {
        let packed = self.value_width().checked_mul(channels)?;
        if self.long {
            packed.div_ceil(80).checked_mul(80)
        } else {
            Some(packed)
        }
    }
}

/// Decodes records of `channels` values from `data` and passes each to `sink`.
///
/// Binary payloads hold exactly `binary_records` records. ASCII payloads end at the first
/// incomplete record. Returns the number of records decoded.
pub(crate) fn decode_records(
    data: &[u8],
    format: Format,
    channels: usize,
    binary_records: usize,
    mut sink: impl FnMut(&[f64]),
) -> Result<usize, Error> {
    let record_size = format.record_size(channels).ok_or(Error::TooLarge)?;
    let width = format.value_width();
    let mut values = vec![0.0; channels];

    if !format.ascii {
        let expected = record_size
            .checked_mul(binary_records)
            .ok_or(Error::TooLarge)?;
        if data.len() < expected {
            return Err(Error::Truncated {
                expected,
                actual: data.len(),
            });
        }
        for record in data[..expected].chunks_exact(record_size) {
            for (value, bytes) in values.iter_mut().zip(record.chunks_exact(width)) {
                *value = if format.double {
                    f64::from_be_bytes(bytes.try_into().unwrap())
                } else {
                    f64::from(f32::from_be_bytes(bytes.try_into().unwrap()))
                };
            }
            sink(&values);
        }
        return Ok(binary_records);
    }

    let text_len = channels * width;
    let mut count = 0;
    let mut pos = 0;
    let mut text = Vec::with_capacity(record_size);
    loop {
        text.clear();
        if format.long {
            while pos < data.len() && is_eol(data[pos]) {
                pos += 1;
            }
            for _ in 0..record_size / 80 {
                if pos >= data.len() {
                    break;
                }
                let (line, next) = split_line(data, pos);
                text.extend_from_slice(line);
                pos = next;
            }
        } else {
            while text.len() < text_len {
                while pos < data.len() && is_eol(data[pos]) {
                    pos += 1;
                }
                let Some(field) = data.get(pos..pos + width) else {
                    break;
                };
                text.extend_from_slice(field);
                pos += width;
            }
        }
        if text.len() < text_len {
            return Ok(count);
        }
        for (value, field) in values.iter_mut().zip(text.chunks_exact(width)) {
            *value = ascii_value(field);
        }
        sink(&values);
        count += 1;
    }
}

fn is_eol(byte: u8) -> bool {
    byte == b'\n' || byte == b'\r'
}

/// Returns the line starting at `pos` without its terminator, and the offset after the
/// terminator. `\r\n`, `\n`, and `\r` each count as one terminator.
pub(crate) fn split_line(data: &[u8], pos: usize) -> (&[u8], usize) {
    let rest = &data[pos..];
    let Some(end) = rest.iter().position(|&b| is_eol(b)) else {
        return (rest, data.len());
    };
    let skip = if rest[end] == b'\r' && rest.get(end + 1) == Some(&b'\n') {
        2
    } else {
        1
    };
    (&rest[..end], pos + end + skip)
}

/// Decodes one fixed-width ASCII field as `decodeRecord` does: a field with any character
/// outside `0-9 + - . e E d D space` is NaN, except `**unused**`, which is 0.
fn ascii_value(field: &[u8]) -> f64 {
    if field.iter().all(|c| b"0123456789+-.eEdD ".contains(c)) {
        std::str::from_utf8(field)
            .ok()
            .and_then(c_atof)
            .unwrap_or(0.0)
    } else if field == b"**unused**" {
        0.0
    } else {
        f64::NAN
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect(
        data: &[u8],
        format: Format,
        channels: usize,
        binary_records: usize,
    ) -> Result<Vec<Vec<f64>>, Error> {
        let mut out = Vec::new();
        decode_records(data, format, channels, binary_records, |r| {
            out.push(r.to_vec())
        })?;
        Ok(out)
    }

    fn fmt(code: &str) -> Format {
        Format::from_code(code)
    }

    #[test]
    fn format_codes() {
        assert_eq!(
            fmt("LRFI"),
            Format {
                long: true,
                double: false,
                ascii: true
            }
        );
        assert_eq!(
            fmt("KDBI"),
            Format {
                long: false,
                double: true,
                ascii: false
            }
        );
        assert_eq!(
            fmt("KRBI                ! x"),
            Format {
                long: false,
                double: false,
                ascii: false
            }
        );
    }

    #[test]
    fn ascii_fields_touch_and_wrap() {
        let data = b" 0.0111111 0.0000000 0.0000000 0.0000000 0.0000000 0.0000000 0.0000000 0.0000000\r\n 1.0000000-0.0111111\r\n\
                     *missing* **unused** 3.0000000 4.0000000 5.0000000 6.0000000 7.0000000 8.0000000\n 9.0000000 1.0D+01   \n";
        let rows = collect(data, fmt("LRFI"), 10, 0).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0][0], 0.0111111);
        assert_eq!(rows[0][9], -0.0111111);
        assert!(rows[1][0].is_nan());
        assert_eq!(rows[1][1], 0.0);
        assert_eq!(rows[1][9], 1.0);
    }

    #[test]
    fn ascii_keeps_final_record_without_newline() {
        let rows = collect(
            b" 1.0000000 2.0000000\n 3.0000000 4.0000000",
            fmt("LRFI"),
            2,
            0,
        )
        .unwrap();
        assert_eq!(rows, vec![vec![1.0, 2.0], vec![3.0, 4.0]]);
    }

    #[test]
    fn ascii_drops_partial_record() {
        let rows = collect(b" 1.0000000 2.0000000\n 3.0000000\n", fmt("LRFI"), 2, 0).unwrap();
        assert_eq!(rows, vec![vec![1.0, 2.0]]);
    }

    #[test]
    fn ascii_double_width() {
        let rows = collect(
            b" 1.23456789012345670-2.0000000000000e-03\n",
            fmt("LDFI"),
            2,
            0,
        )
        .unwrap();
        assert_eq!(rows, vec![vec![1.2345678901234567, -0.002]]);
    }

    #[test]
    fn compact_ascii_ignores_line_breaks_between_fields() {
        let rows = collect(
            b" 1.0000000 2.0000000 3.0000000\n 4.0000000 5.0000000 6.0000000",
            fmt("KRFI"),
            2,
            0,
        )
        .unwrap();
        assert_eq!(rows, vec![vec![1.0, 2.0], vec![3.0, 4.0], vec![5.0, 6.0]]);
    }

    #[test]
    fn binary_single_and_double_are_big_endian() {
        let mut krbi = Vec::new();
        for v in [1.5f32, -2.25, f32::NAN, 4.0] {
            krbi.extend_from_slice(&v.to_be_bytes());
        }
        let rows = collect(&krbi, fmt("KRBI"), 2, 2).unwrap();
        assert_eq!(rows[0], vec![1.5, -2.25]);
        assert!(rows[1][0].is_nan());

        let mut kdbi = Vec::new();
        for v in [0.1f64, 0.2] {
            kdbi.extend_from_slice(&v.to_be_bytes());
        }
        assert_eq!(
            collect(&kdbi, fmt("KDBI"), 1, 2).unwrap(),
            vec![vec![0.1], vec![0.2]]
        );
    }

    #[test]
    fn long_binary_pads_records_to_80_bytes() {
        let mut lrbi = vec![0u8; 160];
        lrbi[0..4].copy_from_slice(&1.0f32.to_be_bytes());
        lrbi[80..84].copy_from_slice(&2.0f32.to_be_bytes());
        assert_eq!(
            collect(&lrbi, fmt("LRBI"), 1, 2).unwrap(),
            vec![vec![1.0], vec![2.0]]
        );
    }

    #[test]
    fn binary_shorter_than_declared_is_an_error() {
        assert_eq!(
            collect(&[0; 7], fmt("KRBI"), 1, 2),
            Err(Error::Truncated {
                expected: 8,
                actual: 7
            })
        );
    }
}
