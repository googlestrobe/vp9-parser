use bitreader::BitReader;

#[test]
fn test_bitreader_position_sanity() {
    let bytes = vec![0xFF, 0xFF, 0xFF, 0xFF]; // 32 bits of 1
    let mut br = BitReader::new(&bytes);

    // Read 3 bits
    assert_eq!(br.position(), 0);
    let _ = br.read_u8(3).unwrap();
    assert_eq!(br.position(), 3);

    // Read 6 bits
    let _ = br.read_u8(6).unwrap();
    assert_eq!(br.position(), 9);

    // Read 7 bits
    let _ = br.read_u8(7).unwrap();
    assert_eq!(br.position(), 16);
}

#[test]
fn test_bitreader_u16_position_bug() {
    let bytes = vec![0xFF; 10]; // All 1s
    let mut br = BitReader::new(&bytes);

    assert_eq!(br.position(), 0);

    // Position 0
    let _ = br.read_bool().unwrap(); // 1 bit
    assert_eq!(br.position(), 1);

    // Read inverse-like structure
    // Sign bit
    let _ = br.read_bool().unwrap(); // 1 bit
    assert_eq!(br.position(), 2);

    // Magnitude (e.g., 6 bits) Using read_u16
    // br.read_u16 returns u16
    let _ = br.read_u16(6).unwrap();
    assert_eq!(br.position(), 8); // 2 + 6 = 8!

    // Magnitude (e.g., 8 bits) Using read_u16
    let _ = br.read_u16(8).unwrap();
    assert_eq!(br.position(), 16); // 8 + 8 = 16!
}
