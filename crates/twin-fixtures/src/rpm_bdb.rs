pub fn rpm_bdb_database(header: &[u8], is_big_endian: bool, has_overflow: bool) -> Vec<u8> {
    let size = 512;
    let has_overflow = has_overflow || header.len() > size - 36;
    let overflow_pages = if has_overflow {
        header.len().div_ceil(size - 26)
    } else {
        0
    };
    let mut bytes = vec![0; (3 + overflow_pages) * size];
    let word = |bytes: &mut [u8], offset, value: u32| {
        bytes[offset..offset + 4].copy_from_slice(&if is_big_endian {
            value.to_be_bytes()
        } else {
            value.to_le_bytes()
        });
    };
    let short = |bytes: &mut [u8], offset, value: u16| {
        bytes[offset..offset + 2].copy_from_slice(&if is_big_endian {
            value.to_be_bytes()
        } else {
            value.to_le_bytes()
        });
    };
    word(&mut bytes, 12, 0x00061561);
    word(&mut bytes, 16, 9);
    word(&mut bytes, 20, size as u32);
    bytes[25] = 8;
    word(&mut bytes, 32, (2 + overflow_pages) as u32);
    word(&mut bytes, 72, 1);
    word(&mut bytes, 76, 1);
    word(&mut bytes, 96, 1);
    word(&mut bytes, 100, 1);
    for number in 1..=2 {
        word(&mut bytes, number * size + 8, number as u32);
        bytes[number * size + 25] = 13;
        short(&mut bytes, number * size + 22, size as u16);
    }
    let root = size;
    let key = size - 5;
    short(&mut bytes, root + 20, 2);
    short(&mut bytes, root + 26, key as u16);
    bytes[root + key] = 1;
    word(&mut bytes, root + key + 1, 1);
    let value_size = if has_overflow { 12 } else { header.len() + 1 };
    let value = key - value_size;
    short(&mut bytes, root + 28, value as u16);
    short(&mut bytes, root + 22, value as u16);
    if has_overflow {
        bytes[root + value] = 3;
        word(&mut bytes, root + value + 4, 3);
        word(&mut bytes, root + value + 8, header.len() as u32);
        for (index, chunk) in header.chunks(size - 26).enumerate() {
            let number = 3 + index;
            let offset = number * size;
            word(&mut bytes, offset + 8, number as u32);
            word(
                &mut bytes,
                offset + 12,
                if index == 0 { 0 } else { (number - 1) as u32 },
            );
            word(
                &mut bytes,
                offset + 16,
                if index + 1 == overflow_pages {
                    0
                } else {
                    (number + 1) as u32
                },
            );
            bytes[offset + 25] = 7;
            short(&mut bytes, offset + 22, chunk.len() as u16);
            bytes[offset + 26..offset + 26 + chunk.len()].copy_from_slice(chunk);
        }
    } else {
        bytes[root + value] = 1;
        bytes[root + value + 1..root + key].copy_from_slice(header);
    }
    bytes
}
