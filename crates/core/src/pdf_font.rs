use lopdf::{dictionary, Document, Object, ObjectId, Stream};

/// Embed a TrueType CJK font into a lopdf Document as a Type0 CIDFont.
/// Returns (Type0 font ObjectId, ToUnicode stream ObjectId).
/// The ToUnicode stream is initially a placeholder; call `update_tounicode`
/// after generating all text to fill in the actual glyph→Unicode mapping.
pub fn embed_cjk_font(doc: &mut Document, font_name: &str, font_data: &[u8]) -> (ObjectId, ObjectId) {
    let font_stream = Stream::new(
        dictionary! {
            "Length1" => Object::Integer(font_data.len() as i64),
        },
        font_data.to_vec(),
    )
    .with_compression(true);
    let font_stream_id = doc.add_object(font_stream);

    let font_descriptor_id = doc.add_object(dictionary! {
        "Type" => "FontDescriptor",
        "FontName" => font_name,
        "Flags" => Object::Integer(4),
        "FontBBox" => Object::Array(vec![
            Object::Integer(-1000), Object::Integer(-500),
            Object::Integer(3000), Object::Integer(1200),
        ]),
        "ItalicAngle" => Object::Integer(0),
        "Ascent" => Object::Integer(1160),
        "Descent" => Object::Integer(-288),
        "CapHeight" => Object::Integer(860),
        "StemV" => Object::Integer(80),
        "FontFile2" => Object::Reference(font_stream_id),
    });

    let cid_font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "CIDFontType2",
        "BaseFont" => font_name,
        "CIDSystemInfo" => dictionary! {
            "Registry" => Object::string_literal("Adobe"),
            "Ordering" => Object::string_literal("Identity"),
            "Supplement" => Object::Integer(0),
        },
        "FontDescriptor" => Object::Reference(font_descriptor_id),
        "DW" => Object::Integer(1000),
        "CIDToGIDMap" => "Identity",
    });

    // Placeholder ToUnicode — will be updated after text generation
    let tounicode_stream = Stream::new(dictionary! {}, build_tounicode_cmap(&[]).into_bytes());
    let tounicode_id = doc.add_object(tounicode_stream);

    let type0_font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type0",
        "BaseFont" => font_name,
        "Encoding" => "Identity-H",
        "DescendantFonts" => Object::Array(vec![Object::Reference(cid_font_id)]),
        "ToUnicode" => Object::Reference(tounicode_id),
    });

    (type0_font_id, tounicode_id)
}

/// Update the ToUnicode CMap stream with actual glyph→Unicode mappings.
pub fn update_tounicode(doc: &mut Document, tounicode_id: ObjectId, glyph_map: &[(u16, char)]) {
    let cmap = build_tounicode_cmap(glyph_map);
    let stream = Stream::new(dictionary! {}, cmap.into_bytes());
    doc.set_object(tounicode_id, stream);
}

fn build_tounicode_cmap(glyph_map: &[(u16, char)]) -> String {
    let mut unique: std::collections::BTreeMap<u16, char> = std::collections::BTreeMap::new();
    for &(gid, unicode) in glyph_map {
        unique.insert(gid, unicode);
    }

    let entries: Vec<_> = unique.iter().collect();

    let mut cmap = String::new();
    cmap.push_str("/CIDInit /ProcSet findresource begin\n");
    cmap.push_str("12 dict begin\n");
    cmap.push_str("begincmap\n");
    cmap.push_str("/CIDSystemInfo\n");
    cmap.push_str("<< /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n");
    cmap.push_str("/CMapName /Adobe-Identity-UCS def\n");
    cmap.push_str("/CMapType 2 def\n");
    cmap.push_str("1 begincodespacerange\n");
    cmap.push_str("<0000> <FFFF>\n");
    cmap.push_str("endcodespacerange\n");

    // Emit in chunks of 100 (PDF spec limit per beginbfchar block)
    for chunk in entries.chunks(100) {
        cmap.push_str(&format!("{} beginbfchar\n", chunk.len()));
        for (gid, unicode) in chunk {
            cmap.push_str(&format!("<{:04X}> <{:04X}>\n", gid, **unicode as u32));
        }
        cmap.push_str("endbfchar\n");
    }

    cmap.push_str("endcmap\n");
    cmap.push_str("CMapName currentdict /CMap defineresource pop\n");
    cmap.push_str("end\n");
    cmap.push_str("end\n");
    cmap
}
