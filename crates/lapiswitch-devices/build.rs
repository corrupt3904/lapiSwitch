//! Reads every `devices/<MODEL>/` folder, validates it and generates the
//! static device table. Any problem fails the build with the file and field.
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use toml_edit::{DocumentMut, Item, Table};

const SHARED_SIZE: i64 = 0x1000;
const ROLLBACK_COUNT: usize = 32;

type Result<T> = std::result::Result<T, String>;

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../devices");
    println!("cargo:rerun-if-changed={}", root.display());
    let code = match generate(&root) {
        Ok(code) => code,
        Err(error) => panic!("\n\ndevices/ 기기 파일 오류:\n{error}\n\n"),
    };
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("devices.rs");
    std::fs::write(out, code).unwrap();
}

fn generate(root: &Path) -> Result<String> {
    let mut folders: Vec<PathBuf> = std::fs::read_dir(root)
        .map_err(|e| format!("{}: {e}", root.display()))?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.is_dir())
        .collect();
    folders.sort();
    let mut code = String::from(
        "/// Every folder in `devices/`, sorted by model.\npub static DEVICES: &[Device] = &[\n",
    );
    let mut flows = 0;
    for folder in &folders {
        for name in ["device.toml", "deviceinfo.toml", "image.png"] {
            println!("cargo:rerun-if-changed={}", folder.join(name).display());
        }
        let model = folder.file_name().unwrap().to_string_lossy().into_owned();
        let (device, read) = device(folder, &model).map_err(|e| format!("{model}: {e}"))?;
        flows += usize::from(read);
        code.push_str(&device);
    }
    code.push_str("];\n");
    if flows == 0 {
        return Err("deviceinfo_read = true인 기기가 하나도 없습니다.".into());
    }
    Ok(code)
}

fn read(path: &Path) -> Result<DocumentMut> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    text.parse::<DocumentMut>()
        .map_err(|e| format!("{}: TOML 형식 오류\n{e}", path.display()))
}

/// Typos must not silently drop a value.
fn only(table: &Table, allowed: &[&str], place: &str) -> Result<()> {
    for (key, _) in table.iter() {
        if !allowed.contains(&key) {
            return Err(format!("{place}: 알 수 없는 항목 `{key}`"));
        }
    }
    Ok(())
}

fn string<'a>(table: &'a Table, key: &str, place: &str) -> Result<&'a str> {
    table
        .get(key)
        .and_then(Item::as_str)
        .ok_or_else(|| format!("{place}: `{key}` 문자열이 필요합니다."))
}

fn int(table: &Table, key: &str, place: &str) -> Result<i64> {
    table
        .get(key)
        .and_then(Item::as_integer)
        .ok_or_else(|| format!("{place}: `{key}` 정수가 필요합니다."))
}

fn table<'a>(doc: &'a Table, key: &str, place: &str) -> Result<&'a Table> {
    doc.get(key)
        .and_then(Item::as_table)
        .ok_or_else(|| format!("{place}: [{key}]가 필요합니다."))
}

const FLOW_KEYS: [&str; 4] = ["firmware", "installed", "twrp", "country"];

fn flag(doc: &DocumentMut, key: &str) -> Result<bool> {
    doc.get(key)
        .and_then(Item::as_bool)
        .ok_or_else(|| format!("device.toml: `{key}`(true/false)가 필요합니다."))
}

fn device(folder: &Path, model: &str) -> Result<(String, bool)> {
    let doc = read(&folder.join("device.toml"))?;
    let place = "device.toml";
    let mut allowed = vec!["model", "codename", "deviceinfo_read", "deviceinfo_write"];
    allowed.extend(FLOW_KEYS);
    only(&doc, &allowed, place)?;
    if string(&doc, "model", place)? != model {
        return Err("device.toml의 model이 폴더 이름과 다릅니다.".into());
    }
    let codename = match doc.get("codename") {
        None => None,
        Some(item) => Some(
            item.as_str()
                .ok_or("device.toml: `codename`은 문자열이어야 합니다.")?,
        ),
    };
    let read = flag(&doc, "deviceinfo_read")?;
    let write = flag(&doc, "deviceinfo_write")?;
    if write && !read {
        return Err(
            "device.toml: deviceinfo_write = true에는 deviceinfo_read = true가 필요합니다.".into(),
        );
    }
    // Reading already runs the backup, TWRP and restore flow.
    let flow_code = if read {
        if codename.is_none() {
            return Err(
                "device.toml: deviceinfo_read = true에는 `codename`이 필요합니다 (TWRP ro.product.device 확인용)."
                    .into(),
            );
        }
        format!("Some({})", flow_data(&doc)?)
    } else {
        for key in FLOW_KEYS {
            if doc.contains_key(key) {
                return Err(format!(
                    "device.toml: deviceinfo_read = false인 기기에는 [{key}]를 두지 않습니다."
                ));
            }
        }
        "None".to_owned()
    };
    let image = folder.join("image.png");
    let image = if image.is_file() {
        let path = image
            .canonicalize()
            .map_err(|e| format!("image.png: {e}"))?;
        format!("Some(include_bytes!({:?}))", path.to_string_lossy())
    } else {
        "None".to_owned()
    };
    // Writing needs the field map; a dump saves the raw bytes without one.
    let map = folder.join("deviceinfo.toml");
    let deviceinfo = if map.is_file() {
        format!("Some({})", deviceinfo(&self::read(&map)?)?)
    } else if write {
        return Err("deviceinfo_write = true에는 deviceinfo.toml이 필요합니다.".into());
    } else {
        "None".to_owned()
    };
    Ok((
        format!(
            "    Device {{\n        model: {model:?},\n        codename: {codename:?},\n        deviceinfo_read: {read},\n        deviceinfo_write: {write},\n        flow: {flow_code},\n        image: {image},\n        deviceinfo: {deviceinfo},\n    }},\n"
        ),
        read,
    ))
}

fn region(value: &str, place: &str) -> Result<&'static str> {
    match value {
        "PRC" => Ok("RegionCode::Prc"),
        "ROW" => Ok("RegionCode::Row"),
        other => Err(format!("{place}: region은 PRC 또는 ROW입니다 ({other}).")),
    }
}

fn firmware(doc: &DocumentMut) -> Result<String> {
    let place = "[firmware]";
    let firmware = table(doc, "firmware", "device.toml")?;
    only(
        firmware,
        &["loader", "required_images", "region_marker"],
        place,
    )?;
    let loader = string(firmware, "loader", place)?;
    let images: Vec<&str> = firmware
        .get("required_images")
        .and_then(Item::as_array)
        .ok_or("[firmware]: `required_images` 배열이 필요합니다.")?
        .iter()
        .map(|v| {
            v.as_str()
                .ok_or("[firmware]: `required_images`는 문자열 배열이어야 합니다.")
        })
        .collect::<std::result::Result<_, _>>()?;
    for required in ["vendor_boot.img", "vbmeta_system.img", "boot.img"] {
        if !images.contains(&required) {
            return Err(format!(
                "[firmware]: `required_images`에 리전·버전 확인용 {required}가 있어야 합니다."
            ));
        }
    }
    let marker = match string(firmware, "region_marker", place)? {
        "region_country" => "RegionMarker::RegionCountry",
        "product_region/build_region" => "RegionMarker::ProductRegionBuildRegion",
        other => {
            return Err(format!(
                "[firmware]: 알 수 없는 region_marker `{other}` (region_country, product_region/build_region)"
            ));
        }
    };
    Ok(format!(
        "FirmwarePackage {{ loader: {loader:?}, required_images: &{images:?}, region_marker: {marker} }}"
    ))
}

fn installed(doc: &DocumentMut) -> Result<String> {
    let place = "[installed]";
    let installed = table(doc, "installed", "device.toml")?;
    only(installed, &["scheme", "allow"], place)?;
    let scheme = string(installed, "scheme", place)?;
    let (scheme_code, keys): (&str, &[&str]) = match scheme {
        "zui_version" => (
            "VersionScheme::ZuiVersion",
            &["region", "family", "max", "label"],
        ),
        "build_date" => (
            "VersionScheme::BuildDate",
            &["region", "suffix", "max", "label"],
        ),
        other => {
            return Err(format!(
                "[installed]: 알 수 없는 scheme `{other}` (zui_version, build_date)"
            ));
        }
    };
    let entries = installed
        .get("allow")
        .and_then(Item::as_array_of_tables)
        .ok_or("[installed]: [[installed.allow]]가 필요합니다.")?;
    let mut allow = String::from("&[");
    for (n, entry) in entries.iter().enumerate() {
        let place = format!("[[installed.allow]] #{}", n + 1);
        only(entry, keys, &place)?;
        let region = region(string(entry, "region", &place)?, &place)?;
        let label = match (entry.get("max"), entry.get("label")) {
            (None, None) => None,
            (Some(_), Some(label)) => Some(
                label
                    .as_str()
                    .ok_or(format!("{place}: `label`은 문자열입니다."))?,
            ),
            _ => return Err(format!("{place}: `max`와 `label`은 함께 두어야 합니다.")),
        };
        let max = entry.get("max").and_then(Item::as_str);
        if entry.contains_key("max") && max.is_none() {
            return Err(format!("{place}: `max`는 문자열입니다."));
        }
        let code = if scheme == "zui_version" {
            let family = string(entry, "family", &place)?;
            let max = match max {
                None => "None".to_owned(),
                Some(max) => {
                    let numbers: Vec<u32> = max
                        .split('.')
                        .map(|p| {
                            p.parse()
                                .map_err(|_| format!("{place}: `max` 버전 형식 오류 ({max})"))
                        })
                        .collect::<Result<_>>()?;
                    let numbers: [u32; 4] = numbers
                        .try_into()
                        .map_err(|_| format!("{place}: `max`는 숫자 4개(a.b.c.d)여야 합니다."))?;
                    format!("Some(({numbers:?}, {:?}))", label.unwrap())
                }
            };
            format!(
                "InstalledFirmware::Version {{ region: {region}, family: {family:?}, max: {max} }}"
            )
        } else {
            let suffix = string(entry, "suffix", &place)?;
            if suffix.len() != 1 || !suffix.bytes().all(|b| b.is_ascii_uppercase()) {
                return Err(format!(
                    "{place}: `suffix`는 대문자 한 글자입니다 ({suffix})."
                ));
            }
            let max = match max {
                None => "None".to_owned(),
                Some(max) => {
                    let digits: String = max.chars().filter(|c| *c != '-').collect();
                    let valid = max.len() == 10
                        && max.as_bytes()[4] == b'-'
                        && max.as_bytes()[7] == b'-'
                        && digits.len() == 8
                        && digits.bytes().all(|b| b.is_ascii_digit());
                    let date: u32 = digits.parse().unwrap_or(0);
                    let (month, day) = (date / 100 % 100, date % 100);
                    if !valid || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
                        return Err(format!("{place}: `max`는 YYYY-MM-DD 날짜입니다 ({max})."));
                    }
                    format!("Some(({date}, {:?}))", label.unwrap())
                }
            };
            format!(
                "InstalledFirmware::BuildDate {{ region: {region}, suffix: {:?}, max: {max} }}",
                suffix.chars().next().unwrap()
            )
        };
        allow.push_str(&code);
        allow.push_str(", ");
    }
    allow.push(']');
    if entries.is_empty() {
        return Err("[installed]: 허용 항목이 하나 이상 필요합니다.".into());
    }
    Ok(format!("{scheme_code}, installed_firmware: {allow}"))
}

fn twrp(doc: &DocumentMut) -> Result<String> {
    let place = "[twrp]";
    let twrp = table(doc, "twrp", "device.toml")?;
    only(twrp, &["tag", "name", "size", "sha256", "url"], place)?;
    let sha256 = string(twrp, "sha256", place)?;
    if sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("[twrp]: `sha256`은 16진수 64자리여야 합니다.".into());
    }
    let size = int(twrp, "size", place)?;
    if size <= 0 {
        return Err("[twrp]: `size`는 양수여야 합니다.".into());
    }
    let url = string(twrp, "url", place)?;
    if !url.starts_with("https://") {
        return Err("[twrp]: `url`은 https://로 시작해야 합니다.".into());
    }
    Ok(format!(
        "Twrp {{ tag: {:?}, name: {:?}, size: {size}, sha256: {:?}, url: {url:?} }}",
        string(twrp, "tag", place)?,
        string(twrp, "name", place)?,
        sha256.to_ascii_lowercase(),
    ))
}

fn country(doc: &DocumentMut) -> Result<String> {
    let place = "[country]";
    let country = table(doc, "country", "device.toml")?;
    match string(country, "method", place)? {
        "devinfo_persist" => {
            only(country, &["method"], place)?;
            Ok("Country::DevinfoPersist".into())
        }
        "oemowninfo" => {
            only(
                country,
                &["method", "partition", "lun", "size", "offset", "field_size"],
                place,
            )?;
            let partition = string(country, "partition", place)?;
            let lun = int(country, "lun", place)?;
            let size = int(country, "size", place)?;
            let offset = int(country, "offset", place)?;
            let field = int(country, "field_size", place)?;
            if !(0..=7).contains(&lun) {
                return Err("[country]: `lun`은 0..=7입니다.".into());
            }
            if size <= 0 || size % 4096 != 0 || field < 4 || offset < 0 || offset + field > size {
                return Err(
                    "[country]: size는 4096의 배수, field_size ≥ 4, offset + field_size ≤ size여야 합니다."
                        .into(),
                );
            }
            Ok(format!(
                "Country::Oemowninfo(OemOwnInfo {{ partition: {partition:?}, lun: {lun}, size: {size}, offset: {offset}, field_size: {field} }})"
            ))
        }
        other => Err(format!(
            "[country]: 알 수 없는 method `{other}` (devinfo_persist, oemowninfo)"
        )),
    }
}

fn flow_data(doc: &DocumentMut) -> Result<String> {
    Ok(format!(
        "Flow {{ firmware: {}, version_scheme: {}, twrp: {}, country: {} }}",
        firmware(doc)?,
        installed(doc)?,
        twrp(doc)?,
        country(doc)?
    ))
}

struct Field {
    key: String,
    offset: i64,
    size: i64,
    kind: &'static str,
    class: &'static str,
    description: String,
}

fn deviceinfo(doc: &DocumentMut) -> Result<String> {
    only(doc, &["logical_size", "field"], "deviceinfo.toml")?;
    let logical = int(doc, "logical_size", "deviceinfo.toml")?;
    if !(0x0D..=SHARED_SIZE).contains(&logical) {
        return Err(format!(
            "deviceinfo.toml: logical_size는 0x0D..=0x1000이어야 합니다 (0x{logical:X})."
        ));
    }
    let mut fields = vec![];
    let tables = doc
        .get("field")
        .and_then(Item::as_array_of_tables)
        .ok_or("deviceinfo.toml: [[field]]가 없습니다.")?;
    for (n, table) in tables.iter().enumerate() {
        let place = format!(
            "deviceinfo.toml [[field]] #{} ({})",
            n + 1,
            table.get("key").and_then(Item::as_str).unwrap_or("?")
        );
        only(
            table,
            &[
                "key",
                "offset",
                "size",
                "kind",
                "class",
                "description",
                "count",
                "stride",
            ],
            &place,
        )?;
        let key = string(table, "key", &place)?;
        let offset = int(table, "offset", &place)?;
        let size = int(table, "size", &place)?;
        let kind = match string(table, "kind", &place)? {
            "flag" => "Kind::Flag",
            "uint" => "Kind::Uint",
            "region" => "Kind::Region",
            "text" => "Kind::Text",
            "text_nul" => "Kind::TextNul",
            "bytes" => "Kind::Bytes",
            other => return Err(format!("{place}: 알 수 없는 kind `{other}`")),
        };
        let class = match string(table, "class", &place)? {
            "basic" => "Class::Basic",
            "advanced" => "Class::Advanced",
            "protected" => "Class::Protected",
            other => return Err(format!("{place}: 알 수 없는 class `{other}`")),
        };
        let size_ok = match kind {
            "Kind::Flag" => size == 1,
            "Kind::Uint" => matches!(size, 1 | 2 | 4 | 8),
            "Kind::Region" => size == 0x10,
            "Kind::TextNul" => size >= 2,
            _ => size >= 1,
        };
        if !size_ok {
            return Err(format!("{place}: 이 kind에 맞지 않는 size {size}"));
        }
        let description = string(table, "description", &place)?.to_owned();
        let repeated = table.contains_key("count") || table.contains_key("stride");
        if repeated != key.contains("{n}") {
            return Err(format!(
                "{place}: 반복 필드는 key에 {{n}}과 count·stride를 함께 둡니다."
            ));
        }
        let (count, stride) = if repeated {
            (int(table, "count", &place)?, int(table, "stride", &place)?)
        } else {
            (1, 0)
        };
        if count < 1 || (count > 1 && stride < size) {
            return Err(format!("{place}: count ≥ 1, stride ≥ size여야 합니다."));
        }
        for i in 0..count {
            fields.push(Field {
                key: key.replace("{n}", &i.to_string()),
                offset: offset + i * stride,
                size,
                kind,
                class,
                description: description.clone(),
            });
        }
    }
    fields.sort_by_key(|f| f.offset);
    let mut next = 0;
    for field in &fields {
        if field.offset != next {
            return Err(format!(
                "deviceinfo.toml: `{}` 0x{:04X} — 0x{next:04X}부터 이어져야 합니다 (빈틈 또는 겹침).",
                field.key, field.offset
            ));
        }
        next += field.size;
    }
    if next != logical {
        return Err(format!(
            "deviceinfo.toml: 필드가 0x{next:X}에서 끝나지만 logical_size는 0x{logical:X}입니다."
        ));
    }
    let mut keys: Vec<&str> = fields.iter().map(|f| f.key.as_str()).collect();
    keys.sort_unstable();
    if let Some(pair) = keys.windows(2).find(|pair| pair[0] == pair[1]) {
        return Err(format!("deviceinfo.toml: 중복된 key `{}`", pair[0]));
    }
    let find = |key: &str| fields.iter().find(|f| f.key == key);
    let require = |key: &str, kind: &str, size: Option<i64>| -> Result<&Field> {
        let field = find(key).ok_or(format!("deviceinfo.toml: 필수 필드 `{key}`가 없습니다."))?;
        if field.kind != kind || size.is_some_and(|size| field.size != size) {
            return Err(format!(
                "deviceinfo.toml: 필수 필드 `{key}`의 kind·size가 맞지 않습니다."
            ));
        }
        Ok(field)
    };
    if require("magic", "Kind::Text", Some(0x0D))?.offset != 0 {
        return Err("deviceinfo.toml: `magic`은 0x0000에 있어야 합니다.".into());
    }
    require("is_unlocked", "Kind::Uint", Some(1))?;
    require("is_unlock_critical", "Kind::Uint", Some(1))?;
    require("region", "Kind::Region", None)?;
    let start = require("rollback_index[0]", "Kind::Uint", Some(8))?.offset;
    for n in 0..ROLLBACK_COUNT {
        if require(&format!("rollback_index[{n}]"), "Kind::Uint", Some(8))?.offset
            != start + 8 * n as i64
        {
            return Err(
                "deviceinfo.toml: rollback_index[0..31]은 8바이트씩 연속이어야 합니다.".into(),
            );
        }
    }
    let mut code = format!("DeviceInfoMap {{ logical_size: {logical}, fields: &[\n");
    for f in &fields {
        writeln!(
            code,
            "            Field {{ key: {:?}, offset: {}, size: {}, kind: {}, class: {}, description: {:?} }},",
            f.key, f.offset, f.size, f.kind, f.class, f.description
        )
        .unwrap();
    }
    code.push_str("        ] }");
    Ok(code)
}
