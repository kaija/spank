//! Pure decoders for the SPU HID reports (spike report S1/S2).

/// Lid angle feature report `[0x01, lo, hi]` → degrees.
pub fn lid_angle(report: &[u8]) -> Option<u16> {
    match report {
        [0x01, lo, hi, ..] => Some(u16::from_le_bytes([*lo, *hi])),
        _ => None,
    }
}

/// Minimum accelerometer report length (x/y/z int32 at offsets 6/10/14).
pub const ACCEL_REPORT_MIN: usize = 18;

/// Accelerometer input report (22 bytes) → (x, y, z) in g. Never panics.
pub fn accel_xyz(report: &[u8]) -> Option<[f32; 3]> {
    if report.len() < ACCEL_REPORT_MIN {
        return None;
    }
    let rd = |o: usize| {
        let b: [u8; 4] = report.get(o..o + 4)?.try_into().ok()?;
        Some(i32::from_le_bytes(b) as f32 / 65536.0)
    };
    Some([rd(6)?, rd(10)?, rd(14)?])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lid_reports() {
        assert_eq!(lid_angle(&[0x01, 108, 0]), Some(108));
        assert_eq!(lid_angle(&[0x01, 0x2c, 0x01, 0, 0]), Some(300));
        assert_eq!(lid_angle(&[0x02, 108, 0]), None);
        assert_eq!(lid_angle(&[0x01, 108]), None);
        assert_eq!(lid_angle(&[]), None);
    }

    fn report(x: i32, y: i32, z: i32) -> Vec<u8> {
        let mut r = vec![0u8; 22];
        r[0] = 7; // sequence number
        r[6..10].copy_from_slice(&x.to_le_bytes());
        r[10..14].copy_from_slice(&y.to_le_bytes());
        r[14..18].copy_from_slice(&z.to_le_bytes());
        r
    }

    #[test]
    fn accel_reports() {
        // Resting flat: z ≈ -1 g.
        let r = report(1285, -4626, -65003);
        let [x, y, z] = accel_xyz(&r).unwrap();
        assert!((x - 0.0196).abs() < 1e-3);
        assert!((y + 0.0706).abs() < 1e-3);
        assert!((z + 0.9919).abs() < 1e-3);
        assert_eq!(accel_xyz(&r[..18]).map(|v| v[2]), Some(z));
        assert_eq!(accel_xyz(&r[..17]), None);
        assert_eq!(accel_xyz(&[]), None);
        assert_eq!(accel_xyz(&report(i32::MIN, 0, i32::MAX)).unwrap()[0], -32768.0);
    }
}
