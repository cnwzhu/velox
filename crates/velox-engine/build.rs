//! `velox-engine` 构建脚本：按需生成 `wasm32` 轻量 TrueType 字体子集与 `musl` 链接桩。

use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::Path;


#[inline]
fn read_u16_be(b: &[u8], off: usize) -> u16 {
    u16::from_be_bytes([b[off], b[off + 1]])
}

#[inline]
fn read_i16_be(b: &[u8], off: usize) -> i16 {
    i16::from_be_bytes([b[off], b[off + 1]])
}

#[inline]
fn read_u32_be(b: &[u8], off: usize) -> u32 {
    u32::from_be_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}

/// 从单字体 `.ttf` 字节流中按需裁剪 `glyf`/`loca` 表并剥离非渲染表，生成供 `wasm32` `fontdue` 运行时矢量光栅化的超轻量 TrueType 子集（约 30~45 KB）。
fn build_wasm_subset_ttf(extra_chars: &BTreeSet<char>) -> Vec<u8> {
    let ttf_candidates = [
        "C:\\Windows\\Fonts\\segoeui.ttf",
        "C:\\Windows\\Fonts\\arial.ttf",
        "C:\\Windows\\Fonts\\verdana.ttf",
        "C:\\Windows\\Fonts\\trebuc.ttf",
        "/System/Library/Fonts/Supplemental/Arial.ttf",
        "/System/Library/Fonts/Supplemental/Verdana.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/TTF/DejaVuSans.ttf",
        "/usr/share/fonts/noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/liberation/LiberationSans-Regular.ttf",
    ];

    let mut raw_ttf: Option<(Vec<u8>, fontdue::Font)> = None;
    for path in ttf_candidates {
        if let Ok(bytes) = fs::read(path) {
            if bytes.len() >= 12 && &bytes[0..4] == &[0x00, 0x01, 0x00, 0x00] {
                if let Ok(font) =
                    fontdue::Font::from_bytes(bytes.as_slice(), fontdue::FontSettings::default())
                {
                    raw_ttf = Some((bytes, font));
                    break;
                }
            }
        }
    }

    let Some((data, font)) = raw_ttf else {
        return Vec::new();
    };

    let num_tables = read_u16_be(&data, 4) as usize;
    let mut tables: std::collections::BTreeMap<[u8; 4], &[u8]> = std::collections::BTreeMap::new();
    for i in 0..num_tables {
        let rec = 12 + i * 16;
        if rec + 16 > data.len() {
            return Vec::new();
        }
        let tag = [data[rec], data[rec + 1], data[rec + 2], data[rec + 3]];
        let toff = read_u32_be(&data, rec + 8) as usize;
        let tlen = read_u32_be(&data, rec + 12) as usize;
        if toff + tlen <= data.len() {
            tables.insert(tag, &data[toff..toff + tlen]);
        }
    }

    let (Some(&head), Some(&maxp), Some(&loca), Some(&glyf)) = (
        tables.get(b"head"),
        tables.get(b"maxp"),
        tables.get(b"loca"),
        tables.get(b"glyf"),
    ) else {
        return Vec::new();
    };

    if head.len() < 54 || maxp.len() < 6 {
        return Vec::new();
    }
    let index_to_loc_format = read_i16_be(head, 50);
    let num_glyphs = read_u16_be(maxp, 4) as usize;

    let mut offsets: Vec<usize> = Vec::with_capacity(num_glyphs + 1);
    if index_to_loc_format == 0 {
        if loca.len() < (num_glyphs + 1) * 2 {
            return Vec::new();
        }
        for i in 0..=num_glyphs {
            offsets.push((read_u16_be(loca, i * 2) as usize) * 2);
        }
    } else {
        if loca.len() < (num_glyphs + 1) * 4 {
            return Vec::new();
        }
        for i in 0..=num_glyphs {
            offsets.push(read_u32_be(loca, i * 4) as usize);
        }
    }

    let mut keep_gids: BTreeSet<u16> = BTreeSet::new();
    keep_gids.insert(0);
    for cp in (0x0020u32..=0x00FF)
        .chain(0x2000u32..=0x206F)
        .chain(0x2190u32..=0x21FF)
    {
        if let Some(ch) = char::from_u32(cp) {
            let gid = font.lookup_glyph_index(ch);
            if gid > 0 {
                keep_gids.insert(gid);
            }
        }
    }
    for &ch in extra_chars {
        let gid = font.lookup_glyph_index(ch);
        if gid > 0 {
            keep_gids.insert(gid);
        }
    }

    // 递归包含复合字形（Composite Glyph）引用的子组件字形 ID
    let mut worklist: Vec<u16> = keep_gids.iter().copied().collect();
    while let Some(gid) = worklist.pop() {
        let g = gid as usize;
        if g >= num_glyphs {
            continue;
        }
        let (s, e) = (offsets[g], offsets[g + 1]);
        if e > s && e <= glyf.len() && e - s >= 10 {
            let num_contours = read_i16_be(glyf, s);
            if num_contours < 0 {
                let mut pos = s + 10;
                while pos + 4 <= e {
                    let flags = read_u16_be(glyf, pos);
                    let comp_gid = read_u16_be(glyf, pos + 2);
                    if keep_gids.insert(comp_gid) {
                        worklist.push(comp_gid);
                    }
                    pos += 4;
                    if (flags & 0x0001) != 0 {
                        pos += 4;
                    } else {
                        pos += 2;
                    }
                    if (flags & 0x0008) != 0 {
                        pos += 2;
                    } else if (flags & 0x0040) != 0 {
                        pos += 4;
                    } else if (flags & 0x0080) != 0 {
                        pos += 8;
                    }
                    if (flags & 0x0020) == 0 {
                        break;
                    }
                }
            }
        }
    }

    let mut new_glyf: Vec<u8> = Vec::with_capacity(32 * 1024);
    let mut new_loca: Vec<u8> = Vec::with_capacity((num_glyphs + 1) * 4);
    new_loca.extend_from_slice(&0u32.to_be_bytes());
    for gid in 0..num_glyphs {
        if keep_gids.contains(&(gid as u16)) {
            let (s, e) = (offsets[gid], offsets[gid + 1]);
            if e > s && e <= glyf.len() {
                new_glyf.extend_from_slice(&glyf[s..e]);
                while (new_glyf.len() & 3) != 0 {
                    new_glyf.push(0);
                }
            }
        }
        new_loca.extend_from_slice(&(new_glyf.len() as u32).to_be_bytes());
    }

    let mut new_head = head.to_vec();
    // 统一使用 32-bit loca 偏移表（indexToLocFormat = 1）
    new_head[50] = 0;
    new_head[51] = 1;

    let keep_tags: [&[u8; 4]; 8] = [
        b"OS/2", b"cmap", b"glyf", b"head", b"hhea", b"hmtx", b"loca", b"maxp",
    ];
    let mut out_tables: Vec<([u8; 4], Vec<u8>)> = Vec::with_capacity(8);
    for &tag in &keep_tags {
        let bytes = match tag {
            b"glyf" => new_glyf.clone(),
            b"loca" => new_loca.clone(),
            b"head" => new_head.clone(),
            _ => match tables.get(tag) {
                Some(&slice) => slice.to_vec(),
                None => return Vec::new(),
            },
        };
        out_tables.push((*tag, bytes));
    }

    let table_count = out_tables.len() as u16;
    let mut max_pow2 = 1u16;
    let mut entry_selector = 0u16;
    while (max_pow2 << 1) <= table_count {
        max_pow2 <<= 1;
        entry_selector += 1;
    }
    let search_range = max_pow2 * 16;
    let range_shift = table_count * 16 - search_range;

    let mut out_ttf: Vec<u8> = Vec::with_capacity(48 * 1024);
    out_ttf.extend_from_slice(&0x00010000u32.to_be_bytes());
    out_ttf.extend_from_slice(&table_count.to_be_bytes());
    out_ttf.extend_from_slice(&search_range.to_be_bytes());
    out_ttf.extend_from_slice(&entry_selector.to_be_bytes());
    out_ttf.extend_from_slice(&range_shift.to_be_bytes());

    let dir_end = 12 + (table_count as usize) * 16;
    out_ttf.resize(dir_end, 0);

    for (idx, (tag, tbytes)) in out_tables.iter().enumerate() {
        let toff = out_ttf.len() as u32;
        let tlen = tbytes.len() as u32;
        out_ttf.extend_from_slice(tbytes);
        while (out_ttf.len() & 3) != 0 {
            out_ttf.push(0);
        }
        let rec = 12 + idx * 16;
        out_ttf[rec..rec + 4].copy_from_slice(tag);
        out_ttf[rec + 4..rec + 8].copy_from_slice(&0u32.to_be_bytes());
        out_ttf[rec + 8..rec + 12].copy_from_slice(&toff.to_be_bytes());
        out_ttf[rec + 12..rec + 16].copy_from_slice(&tlen.to_be_bytes());
    }

    if fontdue::Font::from_bytes(out_ttf.as_slice(), fontdue::FontSettings::default()).is_ok() {
        out_ttf
    } else {
        Vec::new()
    }
}


fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let char_set: BTreeSet<char> = BTreeSet::new();

    let out_dir = env::var("OUT_DIR").expect("OUT_DIR not set");
    if env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("musl") {
        let libdl_path = Path::new(&out_dir).join("libdl.a");
        let _ = fs::write(&libdl_path, b"!<arch>\n");
        println!("cargo:rustc-link-search=native={out_dir}");
    }

    let wasm_ttf_path = Path::new(&out_dir).join("wasm_subset.ttf");
    if env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("wasm32") {
        let subset_bytes = build_wasm_subset_ttf(&char_set);
        fs::write(&wasm_ttf_path, &subset_bytes).expect("Unable to write wasm_subset.ttf");
    } else if !wasm_ttf_path.exists() {
        fs::write(&wasm_ttf_path, &[]).expect("Unable to write empty wasm_subset.ttf");
    }

}
