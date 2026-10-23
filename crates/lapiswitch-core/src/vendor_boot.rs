//! Patch only the region property of each tree in the declared concatenated
//! DTB area: root `region_country`, or `build_region` of `product_region`
//! (`region_marker` in device.toml). No repacking, fixed offsets, signature
//! replacement or global string search.
use crate::region::Region;
pub use lapiswitch_devices::RegionMarker;

fn word(b: &[u8], at: usize, big: bool) -> Result<usize, String> {
    let a: [u8; 4] = b
        .get(at..at.checked_add(4).ok_or("offset overflow")?)
        .ok_or(crate::msg!("core_vb_truncated_field"))?
        .try_into()
        .unwrap();
    Ok(if big {
        u32::from_be_bytes(a)
    } else {
        u32::from_le_bytes(a)
    } as usize)
}
fn end(start: usize, len: usize, limit: usize) -> Result<usize, String> {
    start
        .checked_add(len)
        .filter(|e| *e <= limit)
        .ok_or_else(|| crate::msg!("core_vb_range"))
}
fn aligned(n: usize, unit: usize) -> Result<usize, String> {
    Ok(n.checked_add(unit - 1)
        .ok_or(crate::msg!("core_vb_align_overflow"))?
        / unit
        * unit)
}
fn offsets(marker: RegionMarker, b: &[u8]) -> Result<Vec<usize>, String> {
    if b.get(..8) != Some(b"VNDRBOOT") {
        return Err(crate::msg!("core_vb_magic"));
    }
    let version = word(b, 8, false)?;
    let expected = match version {
        3 => 2112,
        4 => 2128,
        _ => return Err(crate::msg!("core_vb_version")),
    };
    let page = word(b, 12, false)?;
    let header = word(b, 2096, false)?;
    if !(2048..=65536).contains(&page)
        || !page.is_power_of_two()
        || header != expected
        || b.len() < header
    {
        return Err(crate::msg!("core_vb_header"));
    }
    let start = end(
        aligned(header, page)?,
        aligned(word(b, 24, false)?, page)?,
        b.len(),
    )?;
    let limit = end(start, word(b, 2100, false)?, b.len())?;
    if limit == start {
        return Err(crate::msg!("core_vb_no_dtb"));
    }
    let mut base = start;
    let mut found = vec![];
    let mut trees = 0;
    while base < limit {
        if b[base..limit].iter().all(|v| *v == 0) {
            break;
        }
        trees += 1;
        if trees > 256 || word(b, base, true)? != 0xd00dfeed {
            return Err(crate::msg!("core_vb_dtb_magic"));
        }
        let size = word(b, base + 4, true)?;
        let tree_end = end(base, size, limit)?;
        if size < 40 || word(b, base + 20, true)? != 17 || word(b, base + 24, true)? > 17 {
            return Err(crate::msg!("core_vb_dtb_header"));
        }
        let structure = word(b, base + 8, true)?;
        let strings = word(b, base + 12, true)?;
        let strings_size = word(b, base + 32, true)?;
        let struct_size = word(b, base + 36, true)?;
        if structure < 40 || strings < 40 || !structure.is_multiple_of(4) {
            return Err(crate::msg!("core_vb_dtb_block"));
        }
        let s_end = end(structure, struct_size, size)?;
        let n_end = end(strings, strings_size, size)?;
        if structure < n_end && strings < s_end {
            return Err(crate::msg!("core_vb_dtb_overlap"));
        }
        let names = &b[base + strings..base + n_end];
        let mut at = structure;
        let mut depth = 0usize;
        let mut nodes: Vec<Vec<u8>> = vec![];
        let mut product_nodes = 0;
        let mut root = false;
        let mut done = false;
        let mut region_seen = false;
        while at < s_end {
            end(at, 4, s_end)?;
            let tag = word(b, base + at, true)?;
            at += 4;
            match tag {
                1 => {
                    let z = b[base + at..base + s_end]
                        .iter()
                        .position(|v| *v == 0)
                        .ok_or(crate::msg!("core_vb_node_end"))?;
                    if depth == 0 {
                        if root || z != 0 {
                            return Err(crate::msg!("core_vb_root_node"));
                        }
                        root = true;
                    }
                    let node = b[base + at..base + at + z].to_vec();
                    if marker == RegionMarker::ProductRegionBuildRegion
                        && depth == 1
                        && node == b"product_region"
                    {
                        product_nodes += 1;
                        if product_nodes > 1 {
                            return Err(crate::msg!("core_vb_duplicate_region_node"));
                        }
                    }
                    nodes.push(node);
                    depth += 1;
                    at = aligned(at + z + 1, 4)?;
                }
                2 => {
                    depth = depth
                        .checked_sub(1)
                        .ok_or(crate::msg!("core_vb_node_depth"))?;
                    nodes.pop();
                }
                3 => {
                    if depth == 0 {
                        return Err(crate::msg!("core_vb_property_location"));
                    }
                    end(at, 8, s_end)?;
                    let len = word(b, base + at, true)?;
                    let name = word(b, base + at + 4, true)?;
                    at += 8;
                    let value_end = end(at, len, s_end)?;
                    let tail = names
                        .get(name..)
                        .ok_or(crate::msg!("core_vb_property_name_range"))?;
                    let z = tail
                        .iter()
                        .position(|v| *v == 0)
                        .ok_or(crate::msg!("core_vb_property_name_end"))?;
                    let is_region = match marker {
                        RegionMarker::RegionCountry => &tail[..z] == b"region_country",
                        RegionMarker::ProductRegionBuildRegion => {
                            &tail[..z] == b"build_region"
                                && depth == 2
                                && nodes[1] == b"product_region"
                        }
                    };
                    if is_region {
                        if depth
                            != (if marker == RegionMarker::RegionCountry {
                                1
                            } else {
                                2
                            })
                            || region_seen
                            || len != 4
                            || b[base + at + 3] != 0
                            || !matches!(&b[base + at..base + at + 3], b"PRC" | b"ROW")
                        {
                            return Err(crate::msg!("core_vb_region_property"));
                        }
                        region_seen = true;
                        found.push(base + at);
                    }
                    at = aligned(value_end, 4)?;
                }
                4 => {}
                9 if root && depth == 0 => {
                    done = true;
                    break;
                }
                _ => return Err(crate::msg!("core_vb_struct_token")),
            }
            if at > s_end {
                return Err(crate::msg!("core_vb_struct_truncated"));
            }
        }
        if !done {
            return Err(crate::msg!("core_vb_struct_end"));
        }
        base = tree_end;
    }
    if found.is_empty() {
        return Err(crate::msg!("core_vb_no_region"));
    }
    Ok(found)
}
pub fn region(marker: RegionMarker, b: &[u8]) -> Result<Region, String> {
    let found = offsets(marker, b)?;
    let token = &b[found[0]..found[0] + 3];
    if found.iter().any(|at| &b[*at..*at + 3] != token) {
        return Err(crate::msg!("core_vb_region_differs"));
    }
    Ok(if token == b"PRC" {
        Region::Prc
    } else {
        Region::Row
    })
}
pub fn patch(marker: RegionMarker, b: &[u8], target: Region) -> Result<Vec<u8>, String> {
    region(marker, b)?;
    let mut out = b.to_vec();
    for at in offsets(marker, b)? {
        out[at..at + 3].copy_from_slice(target.token().as_bytes());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    const LAPIS: RegionMarker = RegionMarker::RegionCountry;
    pub fn image() -> Vec<u8> {
        let mut structure = vec![];
        for n in [1u32, 0, 3, 4, 0] {
            structure.extend(n.to_be_bytes());
        }
        structure.extend(b"PRC\0");
        structure.extend(2u32.to_be_bytes());
        structure.extend(9u32.to_be_bytes());
        let strings = b"region_country\0";
        let size = 40 + structure.len() + strings.len();
        let mut tree = vec![];
        for n in [
            0xd00dfeed,
            size as u32,
            40,
            (40 + structure.len()) as u32,
            40,
            17,
            16,
            0,
            strings.len() as u32,
            structure.len() as u32,
        ] {
            tree.extend(n.to_be_bytes());
        }
        tree.extend(structure);
        tree.extend(strings);
        let mut b = vec![0; 16384];
        b[..8].copy_from_slice(b"VNDRBOOT");
        for (o, v) in [
            (8, 4u32),
            (12, 4096),
            (2096, 2128),
            (2100, (tree.len() * 2) as u32),
        ] {
            b[o..o + 4].copy_from_slice(&v.to_le_bytes());
        }
        b[4096..4096 + tree.len()].copy_from_slice(&tree);
        b[4096 + tree.len()..4096 + 2 * tree.len()].copy_from_slice(&tree);
        b[12000..12004].copy_from_slice(b"PRC\0");
        b
    }
    /// One tree: `/product_region { build_region = "PRC"; }` and a decoy
    /// root `build_region` that must be ignored.
    fn product_region_image() -> Vec<u8> {
        let mut structure = vec![];
        let word = |s: &mut Vec<u8>, n: u32| s.extend(n.to_be_bytes());
        word(&mut structure, 1);
        word(&mut structure, 0);
        word(&mut structure, 1);
        structure.extend(b"product_region\0\0");
        word(&mut structure, 3);
        word(&mut structure, 4);
        word(&mut structure, 0);
        structure.extend(b"PRC\0");
        word(&mut structure, 2);
        word(&mut structure, 3);
        word(&mut structure, 4);
        word(&mut structure, 0);
        structure.extend(b"XYZ\0");
        word(&mut structure, 2);
        word(&mut structure, 9);
        let strings = b"build_region\0";
        let size = 40 + structure.len() + strings.len();
        let mut tree = vec![];
        for n in [
            0xd00dfeed,
            size as u32,
            40,
            (40 + structure.len()) as u32,
            40,
            17,
            16,
            0,
            strings.len() as u32,
            structure.len() as u32,
        ] {
            tree.extend(n.to_be_bytes());
        }
        tree.extend(structure);
        tree.extend(strings);
        let mut b = vec![0; 16384];
        b[..8].copy_from_slice(b"VNDRBOOT");
        for (o, v) in [
            (8, 4u32),
            (12, 4096),
            (2096, 2128),
            (2100, tree.len() as u32),
        ] {
            b[o..o + 4].copy_from_slice(&v.to_le_bytes());
        }
        b[4096..4096 + tree.len()].copy_from_slice(&tree);
        b
    }
    #[test]
    fn each_marker_reads_only_its_own_property() {
        let mojito = RegionMarker::ProductRegionBuildRegion;
        let b = product_region_image();
        assert_eq!(region(mojito, &b).unwrap(), Region::Prc);
        assert!(region(LAPIS, &b).is_err());
        let out = patch(mojito, &b, Region::Row).unwrap();
        assert_eq!(region(mojito, &out).unwrap(), Region::Row);
        assert_eq!(b.iter().zip(&out).filter(|(x, y)| x != y).count(), 3);
        assert!(region(mojito, &image()).is_err());
    }
    #[test]
    fn declared_trees_only_and_all_other_bytes_are_preserved() {
        let original = image();
        let out = patch(LAPIS, &original, Region::Row).unwrap();
        assert_eq!(region(LAPIS, &out).unwrap(), Region::Row);
        assert_eq!(original.iter().zip(&out).filter(|(a, b)| a != b).count(), 6);
        assert_eq!(&out[12000..12004], b"PRC\0");
        assert_eq!(patch(LAPIS, &out, Region::Prc).unwrap(), original);
    }
    #[test]
    fn malformed_or_mixed_trees_never_produce_a_patch() {
        let b = image();
        for len in [0, 8, 2098, 4096, 4140] {
            assert!(patch(LAPIS, &b[..len], Region::Row).is_err());
        }
        let mut bad = b.clone();
        bad[2100..2104].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(patch(LAPIS, &bad, Region::Row).is_err());
        let at = offsets(LAPIS, &b).unwrap();
        let mut bad = b.clone();
        bad[at[1]..at[1] + 3].copy_from_slice(b"ROW");
        assert!(patch(LAPIS, &bad, Region::Row).is_err());
        let mut bad = b;
        bad[at[0] + 3] = 1;
        assert!(patch(LAPIS, &bad, Region::Row).is_err());
    }
    #[test]
    #[ignore = "Read-only local firmware samples; set LAPISWITCH_VENDOR_BOOT_SAMPLE_DIR"]
    fn supplied_prc_row_pair_differs_only_in_the_parsed_properties() {
        let dir = std::path::PathBuf::from(
            std::env::var_os("LAPISWITCH_VENDOR_BOOT_SAMPLE_DIR").unwrap(),
        );
        let prc = std::fs::read(dir.join("vendor_boot_PRC.img")).unwrap();
        let row = std::fs::read(dir.join("vendor_boot_ROW.img")).unwrap();
        assert_eq!(offsets(LAPIS, &prc).unwrap().len(), 4);
        assert_eq!(patch(LAPIS, &prc, Region::Row).unwrap(), row);
        assert_eq!(patch(LAPIS, &row, Region::Prc).unwrap(), prc);
    }
}
