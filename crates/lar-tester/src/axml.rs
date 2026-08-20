const XML_TYPE: u16 = 0x0003;
const STRING_POOL_TYPE: u16 = 0x0001;
const XML_START_NAMESPACE_TYPE: u16 = 0x0100;
const XML_END_NAMESPACE_TYPE: u16 = 0x0101;
const XML_START_ELEMENT_TYPE: u16 = 0x0102;
const XML_END_ELEMENT_TYPE: u16 = 0x0103;
const XML_CDATA_TYPE: u16 = 0x0104;
const XML_RESOURCE_MAP_TYPE: u16 = 0x0180;
const UTF8_FLAG: u32 = 1 << 8;
const TYPE_STRING: u8 = 0x03;
const TYPE_INT_DEC: u8 = 0x10;
const TYPE_INT_HEX: u8 = 0x11;
const NO_INDEX: u32 = u32::MAX;
const ANDROID_NAMESPACE: &str = "http://schemas.android.com/apk/res/android";
const MAX_XML_SIZE: usize = 16 * 1024 * 1024;
const MAX_STRINGS: usize = 100_000;
const MAX_CHUNKS: usize = 100_000;
const MAX_ATTRIBUTES: usize = 4_096;
const MAX_DEPTH: usize = 256;

pub(crate) struct DecodedManifest {
    pub package: Option<String>,
    pub launcher_activity: Option<String>,
    pub min_sdk: Option<u32>,
    pub target_sdk: Option<u32>,
}

struct StringPool {
    strings: Vec<String>,
}

impl StringPool {
    fn get(&self, index: u32) -> Result<&str, &'static str> {
        self.strings
            .get(usize::try_from(index).map_err(|_| "bad string index")?)
            .map(String::as_str)
            .ok_or("bad string index")
    }

    fn optional(&self, index: u32) -> Result<Option<&str>, &'static str> {
        if index == NO_INDEX {
            Ok(None)
        } else {
            self.get(index).map(Some)
        }
    }
}

struct Attribute {
    namespace: u32,
    name: u32,
    raw_value: u32,
    value_type: u8,
    value_data: u32,
}

enum FrameKind {
    Other,
    Activity(Option<String>),
    IntentFilter {
        component: Option<String>,
        main: bool,
        launcher: bool,
    },
}

struct Frame {
    name: u32,
    kind: FrameKind,
}

pub(crate) fn decode_manifest(data: &[u8]) -> Result<DecodedManifest, &'static str> {
    if data.len() < 8 || data.len() > MAX_XML_SIZE {
        return Err("bad binary manifest size");
    }
    let root_type = read_u16(data, 0)?;
    let root_header = read_u16(data, 2)? as usize;
    let root_size = read_u32(data, 4)? as usize;
    if root_type != XML_TYPE || root_header != 8 || root_size != data.len() {
        return Err("bad binary manifest header");
    }

    let mut offset = root_header;
    let mut chunks = 0usize;
    let mut pool = None;
    let mut frames = Vec::new();
    let mut result = DecodedManifest {
        package: None,
        launcher_activity: None,
        min_sdk: None,
        target_sdk: None,
    };

    while offset < root_size {
        chunks = chunks.checked_add(1).ok_or("too many XML chunks")?;
        if chunks > MAX_CHUNKS {
            return Err("too many XML chunks");
        }
        let chunk_type = read_u16(data, offset)?;
        let header_size = read_u16(data, offset + 2)? as usize;
        let chunk_size = read_u32(data, offset + 4)? as usize;
        let end = offset
            .checked_add(chunk_size)
            .filter(|end| *end <= root_size)
            .ok_or("bad XML chunk size")?;
        if header_size < 8 || header_size > chunk_size {
            return Err("bad XML chunk header");
        }

        match chunk_type {
            STRING_POOL_TYPE => {
                if pool.is_some() || !frames.is_empty() {
                    return Err("misplaced string pool");
                }
                pool = Some(decode_string_pool(&data[offset..end], header_size)?);
            }
            XML_START_NAMESPACE_TYPE | XML_END_NAMESPACE_TYPE => {
                if header_size != 16 || chunk_size < 24 || pool.is_none() {
                    return Err("bad namespace chunk");
                }
                let strings = pool.as_ref().ok_or("missing string pool")?;
                strings.optional(read_u32(data, offset + 16)?)?;
                strings.get(read_u32(data, offset + 20)?)?;
            }
            XML_START_ELEMENT_TYPE => {
                let strings = pool.as_ref().ok_or("missing string pool")?;
                parse_start_element(
                    data,
                    offset,
                    end,
                    header_size,
                    strings,
                    &mut frames,
                    &mut result,
                )?;
            }
            XML_END_ELEMENT_TYPE => {
                if header_size != 16 || chunk_size < 24 {
                    return Err("bad end element chunk");
                }
                let strings = pool.as_ref().ok_or("missing string pool")?;
                strings.optional(read_u32(data, offset + 16)?)?;
                let name = read_u32(data, offset + 20)?;
                strings.get(name)?;
                let frame = frames.pop().ok_or("unbalanced XML element")?;
                if frame.name != name {
                    return Err("mismatched XML element");
                }
                if let FrameKind::IntentFilter {
                    component,
                    main: true,
                    launcher: true,
                } = frame.kind
                {
                    if result.launcher_activity.is_none() {
                        result.launcher_activity = component;
                    }
                }
            }
            XML_RESOURCE_MAP_TYPE => {
                if header_size != 8 || (chunk_size - header_size) % 4 != 0 || pool.is_none() {
                    return Err("bad resource map chunk");
                }
            }
            XML_CDATA_TYPE => {
                if header_size != 16 || chunk_size < 28 || pool.is_none() {
                    return Err("bad CDATA chunk");
                }
            }
            _ => return Err("unsupported XML chunk"),
        }
        offset = end;
    }

    if pool.is_none() || !frames.is_empty() {
        return Err("incomplete binary manifest");
    }
    Ok(result)
}

fn parse_start_element(
    data: &[u8],
    offset: usize,
    end: usize,
    header_size: usize,
    strings: &StringPool,
    frames: &mut Vec<Frame>,
    result: &mut DecodedManifest,
) -> Result<(), &'static str> {
    if header_size != 16 || end - offset < 36 || frames.len() >= MAX_DEPTH {
        return Err("bad start element chunk");
    }
    strings.optional(read_u32(data, offset + 16)?)?;
    let name_index = read_u32(data, offset + 20)?;
    let name = strings.get(name_index)?;
    let attribute_start = read_u16(data, offset + 24)? as usize;
    let attribute_size = read_u16(data, offset + 26)? as usize;
    let attribute_count = read_u16(data, offset + 28)? as usize;
    if attribute_start < 20 || attribute_size < 20 || attribute_count > MAX_ATTRIBUTES {
        return Err("bad element attributes");
    }
    let attributes_offset = offset
        .checked_add(header_size)
        .and_then(|value| value.checked_add(attribute_start))
        .ok_or("bad element attributes")?;
    let attributes_size = attribute_size
        .checked_mul(attribute_count)
        .ok_or("bad element attributes")?;
    if attributes_offset
        .checked_add(attributes_size)
        .filter(|value| *value <= end)
        .is_none()
    {
        return Err("bad element attributes");
    }

    let mut attributes = Vec::with_capacity(attribute_count);
    for index in 0..attribute_count {
        let position = attributes_offset + index * attribute_size;
        let value_size = read_u16(data, position + 12)?;
        let value_res0 = *data.get(position + 14).ok_or("bad typed value")?;
        if value_size != 8 || value_res0 != 0 {
            return Err("bad typed value");
        }
        let attribute = Attribute {
            namespace: read_u32(data, position)?,
            name: read_u32(data, position + 4)?,
            raw_value: read_u32(data, position + 8)?,
            value_type: *data.get(position + 15).ok_or("bad typed value")?,
            value_data: read_u32(data, position + 16)?,
        };
        strings.optional(attribute.namespace)?;
        strings.get(attribute.name)?;
        strings.optional(attribute.raw_value)?;
        if attribute.value_type == TYPE_STRING {
            strings.get(attribute.value_data)?;
        }
        attributes.push(attribute);
    }

    match name {
        "manifest" if frames.is_empty() => {
            result.package = string_attribute(strings, &attributes, None, "package")?;
        }
        "uses-sdk" => {
            result.min_sdk = integer_attribute(strings, &attributes, "minSdkVersion")?;
            result.target_sdk = integer_attribute(strings, &attributes, "targetSdkVersion")?;
        }
        "action" => {
            if string_attribute(strings, &attributes, Some(ANDROID_NAMESPACE), "name")?.as_deref()
                == Some("android.intent.action.MAIN")
            {
                mark_intent_filter(frames, true)?;
            }
        }
        "category" => {
            if string_attribute(strings, &attributes, Some(ANDROID_NAMESPACE), "name")?.as_deref()
                == Some("android.intent.category.LAUNCHER")
            {
                mark_intent_filter(frames, false)?;
            }
        }
        _ => {}
    }

    let kind = match name {
        "activity" | "activity-alias" => FrameKind::Activity(string_attribute(
            strings,
            &attributes,
            Some(ANDROID_NAMESPACE),
            "name",
        )?),
        "intent-filter" => FrameKind::IntentFilter {
            component: current_component(frames),
            main: false,
            launcher: false,
        },
        _ => FrameKind::Other,
    };
    frames.push(Frame {
        name: name_index,
        kind,
    });
    Ok(())
}

fn current_component(frames: &[Frame]) -> Option<String> {
    frames.iter().rev().find_map(|frame| match &frame.kind {
        FrameKind::Activity(component) => component.clone(),
        _ => None,
    })
}

fn mark_intent_filter(frames: &mut [Frame], main: bool) -> Result<(), &'static str> {
    let frame = frames.last_mut().ok_or("action outside intent-filter")?;
    match &mut frame.kind {
        FrameKind::IntentFilter {
            main: is_main,
            launcher,
            ..
        } => {
            if main {
                *is_main = true;
            } else {
                *launcher = true;
            }
            Ok(())
        }
        _ => Err("action outside intent-filter"),
    }
}

fn string_attribute(
    strings: &StringPool,
    attributes: &[Attribute],
    namespace: Option<&str>,
    name: &str,
) -> Result<Option<String>, &'static str> {
    let Some(attribute) = find_attribute(strings, attributes, namespace, name)? else {
        return Ok(None);
    };
    if let Some(raw) = strings.optional(attribute.raw_value)? {
        return Ok(Some(raw.to_owned()));
    }
    if attribute.value_type == TYPE_STRING {
        return Ok(Some(strings.get(attribute.value_data)?.to_owned()));
    }
    Err("attribute is not a string")
}

fn integer_attribute(
    strings: &StringPool,
    attributes: &[Attribute],
    name: &str,
) -> Result<Option<u32>, &'static str> {
    let Some(attribute) = find_attribute(strings, attributes, Some(ANDROID_NAMESPACE), name)?
    else {
        return Ok(None);
    };
    if matches!(attribute.value_type, TYPE_INT_DEC | TYPE_INT_HEX) {
        return Ok(Some(attribute.value_data));
    }
    let value = if let Some(raw) = strings.optional(attribute.raw_value)? {
        raw
    } else if attribute.value_type == TYPE_STRING {
        strings.get(attribute.value_data)?
    } else {
        return Err("SDK attribute is not an integer");
    };
    value.parse().map(Some).map_err(|_| "bad SDK value")
}

fn find_attribute<'a>(
    strings: &StringPool,
    attributes: &'a [Attribute],
    namespace: Option<&str>,
    name: &str,
) -> Result<Option<&'a Attribute>, &'static str> {
    for attribute in attributes {
        if strings.get(attribute.name)? != name {
            continue;
        }
        let actual_namespace = strings.optional(attribute.namespace)?;
        if actual_namespace == namespace {
            return Ok(Some(attribute));
        }
    }
    Ok(None)
}

fn decode_string_pool(chunk: &[u8], header_size: usize) -> Result<StringPool, &'static str> {
    if header_size != 28 || chunk.len() < header_size {
        return Err("bad string pool header");
    }
    let string_count = read_u32(chunk, 8)? as usize;
    let style_count = read_u32(chunk, 12)? as usize;
    let flags = read_u32(chunk, 16)?;
    let strings_start = read_u32(chunk, 20)? as usize;
    let styles_start = read_u32(chunk, 24)? as usize;
    if string_count > MAX_STRINGS
        || strings_start < header_size
        || strings_start > chunk.len()
        || (styles_start != 0 && (styles_start < strings_start || styles_start > chunk.len()))
    {
        return Err("bad string pool bounds");
    }
    let offsets_size = string_count
        .checked_add(style_count)
        .and_then(|count| count.checked_mul(4))
        .ok_or("bad string pool offsets")?;
    if header_size
        .checked_add(offsets_size)
        .filter(|end| *end <= strings_start)
        .is_none()
    {
        return Err("bad string pool offsets");
    }
    let strings_end = if styles_start == 0 {
        chunk.len()
    } else {
        styles_start
    };
    let mut strings = Vec::with_capacity(string_count);
    for index in 0..string_count {
        let relative = read_u32(chunk, header_size + index * 4)? as usize;
        let position = strings_start
            .checked_add(relative)
            .filter(|position| *position < strings_end)
            .ok_or("bad string offset")?;
        let value = if flags & UTF8_FLAG != 0 {
            decode_utf8_string(chunk, position, strings_end)?
        } else {
            decode_utf16_string(chunk, position, strings_end)?
        };
        strings.push(value);
    }
    Ok(StringPool { strings })
}

fn decode_utf8_string(
    data: &[u8],
    mut position: usize,
    end: usize,
) -> Result<String, &'static str> {
    let (_, next) = read_length8(data, position, end)?;
    position = next;
    let (byte_length, next) = read_length8(data, position, end)?;
    position = next;
    let string_end = position
        .checked_add(byte_length)
        .filter(|value| *value < end)
        .ok_or("bad UTF-8 string")?;
    if data.get(string_end) != Some(&0) {
        return Err("unterminated UTF-8 string");
    }
    std::str::from_utf8(&data[position..string_end])
        .map(str::to_owned)
        .map_err(|_| "invalid UTF-8 string")
}

fn decode_utf16_string(data: &[u8], position: usize, end: usize) -> Result<String, &'static str> {
    let (length, mut position) = read_length16(data, position, end)?;
    let byte_length = length.checked_mul(2).ok_or("bad UTF-16 string")?;
    let string_end = position
        .checked_add(byte_length)
        .filter(|value| value.checked_add(2).is_some_and(|term| term <= end))
        .ok_or("bad UTF-16 string")?;
    if read_u16(data, string_end)? != 0 {
        return Err("unterminated UTF-16 string");
    }
    let mut units = Vec::with_capacity(length);
    while position < string_end {
        units.push(read_u16(data, position)?);
        position += 2;
    }
    String::from_utf16(&units).map_err(|_| "invalid UTF-16 string")
}

fn read_length8(data: &[u8], position: usize, end: usize) -> Result<(usize, usize), &'static str> {
    let first = *data
        .get(position)
        .filter(|_| position < end)
        .ok_or("bad string length")?;
    if first & 0x80 == 0 {
        Ok((first as usize, position + 1))
    } else {
        let second = *data
            .get(position + 1)
            .filter(|_| position + 1 < end)
            .ok_or("bad string length")?;
        Ok((
            (((first & 0x7f) as usize) << 8) | second as usize,
            position + 2,
        ))
    }
}

fn read_length16(data: &[u8], position: usize, end: usize) -> Result<(usize, usize), &'static str> {
    if position
        .checked_add(2)
        .filter(|value| *value <= end)
        .is_none()
    {
        return Err("bad string length");
    }
    let first = read_u16(data, position)?;
    if first & 0x8000 == 0 {
        Ok((first as usize, position + 2))
    } else {
        if position
            .checked_add(4)
            .filter(|value| *value <= end)
            .is_none()
        {
            return Err("bad string length");
        }
        let second = read_u16(data, position + 2)?;
        Ok((
            (((first & 0x7fff) as usize) << 16) | second as usize,
            position + 4,
        ))
    }
}

fn read_u16(data: &[u8], position: usize) -> Result<u16, &'static str> {
    let bytes = data
        .get(position..position + 2)
        .ok_or("truncated binary manifest")?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn read_u32(data: &[u8], position: usize) -> Result<u32, &'static str> {
    let bytes = data
        .get(position..position + 4)
        .ok_or("truncated binary manifest")?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}
