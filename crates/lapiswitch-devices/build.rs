//! Reads every `devices/<MODEL>/` folder, validates it and generates the
//! static device table. Any problem fails the build with the file and field.
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use toml_edit::{DocumentMut, Item, Table};

const SHARED_SIZE: i64 = 0x1000;
/// `devices/<MODEL>/text/<lang>.toml`, in the order of the generated arrays.
const TEXT_LANGUAGES: [&str; 5] = ["en", "ko", "zh", "ja", "ru"];
/// Text sections of the two field groups.
const GROUP_KEYS: [&str; 2] = ["@rollback", "@persistent"];
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
    let folders = model_folders(root)?;
    for lang in TEXT_LANGUAGES {
        let text = root.join("text").join(format!("{lang}.toml"));
        println!("cargo:rerun-if-changed={}", text.display());
    }
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
        format!("Some({})", deviceinfo(&self::read(&map)?, folder)?)
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
    names: [String; 5],
    editable: bool,
    choices: Vec<String>,
    decimal: bool,
    max_digits: i64,
    helps: [String; 5],
}

/// Names and help of one text section in every language, `en` first.
#[derive(Default, Clone)]
struct Texts {
    names: [String; 5],
    helps: [String; 5],
}

/// Model folders under `devices/`; `text/` holds the shared texts.
fn model_folders(root: &Path) -> Result<Vec<PathBuf>> {
    let mut folders: Vec<PathBuf> = std::fs::read_dir(root)
        .map_err(|e| format!("{}: {e}", root.display()))?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.is_dir() && !path.ends_with("text"))
        .collect();
    folders.sort();
    Ok(folders)
}

/// The field keys (repeated fields keep `{n}`) of a device map.
fn map_keys(doc: &DocumentMut) -> Vec<String> {
    doc.get("field")
        .and_then(Item::as_array_of_tables)
        .map(|tables| {
            tables
                .iter()
                .filter_map(|table| table.get("key").and_then(Item::as_str).map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// Reads `devices/text/<lang>.toml`, shared by every device: a table per
/// field key or group with `name` and `help`. Either is one string for every
/// device, or `name.<MODEL>` / `help.<MODEL>` for one device, which wins over
/// the shared one. English names every field of the device; another language
/// may leave an entry out, which shows the English one.
fn texts(folder: &Path, keys: &[String]) -> Result<std::collections::BTreeMap<String, Texts>> {
    let root = folder.parent().expect("devices/<MODEL>");
    let model = folder.file_name().unwrap().to_string_lossy().into_owned();
    let mut models = vec![];
    let mut known: std::collections::BTreeSet<String> =
        GROUP_KEYS.iter().map(|key| (*key).to_owned()).collect();
    for other in model_folders(root)? {
        models.push(other.file_name().unwrap().to_string_lossy().into_owned());
        let map = other.join("deviceinfo.toml");
        if map.is_file() {
            known.extend(map_keys(&read(&map)?));
        }
    }
    let mut all: std::collections::BTreeMap<String, Texts> = keys
        .iter()
        .map(String::as_str)
        .chain(GROUP_KEYS)
        .map(|key| (key.to_owned(), Texts::default()))
        .collect();
    for (index, lang) in TEXT_LANGUAGES.iter().enumerate() {
        let path = root.join("text").join(format!("{lang}.toml"));
        if !path.is_file() {
            if index == 0 {
                return Err(format!("{}이(가) 필요합니다.", path.display()));
            }
            continue;
        }
        let doc = read(&path)?;
        for (key, item) in doc.iter() {
            let place = format!("devices/text/{lang}.toml [{key}]");
            if !known.contains(key) {
                return Err(format!(
                    "{place}: 어느 기기의 deviceinfo.toml에도 없는 key입니다."
                ));
            }
            let table = item
                .as_table()
                .ok_or_else(|| format!("{place}: 표([key])여야 합니다."))?;
            let group = GROUP_KEYS.contains(&key);
            only(
                table,
                if group { &["help"] } else { &["name", "help"] },
                &place,
            )?;
            let mut values = [String::new(), String::new()];
            for (slot, part) in ["name", "help"].iter().enumerate() {
                let Some(item) = table.get(part) else {
                    continue;
                };
                if let Some(text) = item.as_str() {
                    values[slot] = text.trim().to_owned();
                    continue;
                }
                let per_model = item.as_table_like().ok_or_else(|| {
                    format!("{place}: `{part}`는 문자열이거나 `{part}.<모델>` 문자열이어야 합니다.")
                })?;
                for (name, value) in per_model.iter() {
                    if !models.iter().any(|m| m == name) {
                        return Err(format!(
                            "{place}: `{part}.{name}`의 모델 `{name}`이 devices/에 없습니다."
                        ));
                    }
                    let text = value.as_str().ok_or_else(|| {
                        format!("{place}: `{part}.{name}`는 문자열이어야 합니다.")
                    })?;
                    if name == model {
                        values[slot] = text.trim().to_owned();
                    }
                }
            }
            // A key another device has, or a value for another model only.
            let Some(texts) = all.get_mut(key) else {
                continue;
            };
            let [name, help] = values;
            if !name.is_empty() {
                texts.names[index] = name;
            }
            if !help.is_empty() {
                texts.helps[index] = help;
            }
        }
    }
    for key in keys {
        if all[key].names[0].is_empty() {
            return Err(format!(
                "devices/text/en.toml: [{key}]의 name이 {model}에 필요합니다."
            ));
        }
    }
    Ok(all)
}

fn boolean(table: &Table, key: &str, default: bool, place: &str) -> Result<bool> {
    match table.get(key) {
        None => Ok(default),
        Some(item) => item
            .as_bool()
            .ok_or_else(|| format!("{place}: `{key}`는 true/false입니다.")),
    }
}

fn strings(table: &Table, key: &str, place: &str) -> Result<Vec<String>> {
    let Some(item) = table.get(key) else {
        return Ok(vec![]);
    };
    let array = item
        .as_array()
        .ok_or_else(|| format!("{place}: `{key}`는 문자열 배열입니다."))?;
    array
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("{place}: `{key}`는 문자열 배열입니다."))
        })
        .collect()
}

fn deviceinfo(doc: &DocumentMut, folder: &Path) -> Result<String> {
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
    let keys: Vec<String> = tables
        .iter()
        .filter_map(|table| table.get("key").and_then(Item::as_str).map(str::to_owned))
        .collect();
    let texts = texts(folder, &keys)?;
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
                "count",
                "stride",
                "editable",
                "choices",
                "decimal",
                "max_digits",
                "help",
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
            "Kind::Region" => size == 3,
            "Kind::TextNul" => size >= 2,
            _ => size >= 1,
        };
        if !size_ok {
            return Err(format!("{place}: 이 kind에 맞지 않는 size {size}"));
        }
        let text = texts[key].clone();
        let editable = boolean(table, "editable", true, &place)?;
        let decimal = boolean(table, "decimal", false, &place)?;
        if decimal && kind != "Kind::Uint" {
            return Err(format!("{place}: `decimal`은 uint에만 씁니다."));
        }
        let max_digits = match table.get("max_digits") {
            None => 0,
            Some(_) => int(table, "max_digits", &place)?,
        };
        if max_digits != 0 && (!decimal || !(1..=20).contains(&max_digits)) {
            return Err(format!(
                "{place}: `max_digits`는 `decimal = true`와 함께 1..=20으로 씁니다."
            ));
        }
        let choices = strings(table, "choices", &place)?;
        for choice in &choices {
            // Each choice is written exactly as the app shows the value.
            let fits = match kind {
                "Kind::Flag" => matches!(choice.as_str(), "0" | "1"),
                "Kind::Uint" => {
                    let max = if size == 8 {
                        u64::MAX
                    } else {
                        (1u64 << (size * 8)) - 1
                    };
                    if size <= 2 || decimal {
                        choice
                            .parse::<u64>()
                            .is_ok_and(|value| value <= max && value.to_string() == *choice)
                    } else {
                        choice.strip_prefix("0x").is_some_and(|digits| {
                            u64::from_str_radix(digits, 16).is_ok_and(|value| {
                                value <= max && format!("0x{value:X}") == *choice
                            })
                        })
                    }
                }
                "Kind::Text" | "Kind::TextNul" => {
                    let room = size - i64::from(kind == "Kind::TextNul");
                    !choice.is_empty()
                        && choice.len() as i64 <= room
                        && choice.bytes().all(|b| (0x20..=0x7E).contains(&b))
                }
                _ => false,
            };
            if !fits {
                return Err(format!(
                    "{place}: `choices`의 `{choice}`가 이 필드에 맞지 않습니다."
                ));
            }
        }
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
                names: text.names.clone(),
                editable,
                choices: choices.clone(),
                decimal,
                max_digits,
                helps: text.helps.clone(),
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
    let region = require("region", "Kind::Region", Some(3))?.offset;
    // The 13 bytes after the region token stay zero and read-only, so a
    // region is recognized only in its known 16-byte shape.
    let fill = require("region_fill", "Kind::Bytes", Some(13))?;
    if fill.offset != region + 3 || fill.editable {
        return Err(
            "deviceinfo.toml: `region_fill`은 region 바로 뒤 13바이트이고 editable = false여야 합니다."
                .into(),
        );
    }
    for key in ["magic", "region_magic_1", "region_magic_2"] {
        if find(key).is_some_and(|field| field.editable) {
            return Err(format!(
                "deviceinfo.toml: `{key}`는 editable = false여야 합니다."
            ));
        }
    }
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
    let mut code = format!(
        "DeviceInfoMap {{ logical_size: {logical}, rollback_help: {:?}, persistent_help: {:?}, fields: &[\n",
        texts["@rollback"].helps, texts["@persistent"].helps
    );
    for f in &fields {
        writeln!(
            code,
            "            Field {{ key: {:?}, offset: {}, size: {}, kind: {}, class: {}, names: {:?}, editable: {}, choices: &{:?}, decimal: {}, max_digits: {}, helps: {:?} }},",
            f.key, f.offset, f.size, f.kind, f.class, f.names, f.editable, f.choices, f.decimal, f.max_digits, f.helps
        )
        .unwrap();
    }
    code.push_str("        ] }");
    Ok(code)
}
