//! Lenovo PTSTPD ConfigurationQuery client.
//!
//! Resolves a device serial number against the public
//! `getMachineSequenceInfo` endpoint and returns the `data` block as an
//! ordered list of `(key, value)` pairs ready for table rendering. Used
//! by the GUI's device-info popup.

use serde::Deserialize;

use crate::error::{LtboxError, Result};

// Base64-obfuscated so the host doesn't surface in code search; decoded at
// runtime via `obf::reveal` (see `obf.rs` — not security).
const ENDPOINT_B64: &str = "aHR0cHM6Ly9wdHN0cGQubGVub3ZvLmNvbS5jbi9ob21lL0NvbmZpZ3VyYXRpb25RdWVyeS9nZXRNYWNoaW5lU2VxdWVuY2VJbmZv";

/// Display order for known fields. Keys missing from the response are
/// skipped silently so a future schema addition does not break the
/// popup; unknown keys are not surfaced (intentional — keeps the table
/// tight and prevents accidental disclosure of any new private fields).
pub const FIELD_ORDER: &[&str] = &[
    "MachineNo",
    "MachineName",
    "MTM",
    "PackingLotNo",
    "ProductDate",
    "ScanDate",
    "SaleArea",
    "PurchaseDate",
    "Iwsor",
    "SaleOrder",
    "Brand",
    "CheckCode",
    "SN",
    "ProductModel",
    "ProductSeries",
    "ProductSmallClass",
    "ProductBigClass",
    "CataLogID",
    "ProductSeriesID",
    "ProductSmallClassID",
    "ProductBigClassID",
    "CusName",
    "ProductLineID",
    "ProductLineName",
    "MachineClass",
    "BrandCode",
    "Describe",
    "SalePath",
    "ResponsibleParty",
    "FeeMarkID",
    "FeeMarkName",
    "ExportRegionCode",
    "ExportRegionName",
    "download_url",
];

/// Slim representation of the response `data` block. `value` is `None`
/// when the upstream JSON returned `null`, so the renderer can choose
/// to leave the cell blank instead of printing the literal `"null"`.
#[derive(Debug, Clone, Default)]
pub struct MachineInfo {
    pub fields: Vec<(String, Option<String>)>,
    /// Raw `data` block from the response, pretty-printed JSON. Used by
    /// the popup's "copy" button so the user gets the unmodified
    /// upstream representation rather than the GUI's table rendering.
    pub data_pretty: String,
}

/// Tri-state lookup for a single field by JSON shape. Distinguishes
/// "key missing" from "key present, value `null`" so callers can
/// branch on each case (e.g. SaleArea = `null` ⇒ ROW preselect).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldValue {
    /// Key not in `data` at all.
    Missing,
    /// Key present, value was JSON `null`.
    Null,
    /// Key present, value was a (possibly empty) string / number /
    /// other non-null primitive.
    Value(String),
}

impl MachineInfo {
    /// Look up a field by key. Returns the tri-state shape
    /// (missing / null / value). The internal `fields` list only
    /// includes keys that were present in the response, so absence
    /// from the list maps to `Missing`.
    pub fn field(&self, key: &str) -> FieldValue {
        for (k, v) in &self.fields {
            if k == key {
                return match v {
                    Some(s) => FieldValue::Value(s.clone()),
                    None => FieldValue::Null,
                };
            }
        }
        FieldValue::Missing
    }
}

#[derive(Debug, Deserialize)]
struct Envelope {
    #[serde(default, rename = "StatusCode")]
    status_code: Option<i64>,
    #[serde(default, rename = "Message")]
    message: Option<String>,
    #[serde(default)]
    data: Option<serde_json::Value>,
}

/// Blocking GET against the PTSTPD endpoint. Pulls only the fields
/// listed in [`FIELD_ORDER`] in that order.
pub fn fetch_machine_info(serial: &str) -> Result<MachineInfo> {
    let trimmed = serial.trim();
    if trimmed.is_empty() {
        return Err(LtboxError::Other("empty serial".into()));
    }
    let agent = crate::downloader::build_agent();
    // `query` percent-encodes the serial; formatting it into the URL did not.
    let mut resp = agent
        .get(crate::obf::reveal(ENDPOINT_B64))
        .query("MachineNo", trimmed)
        .call()
        .map_err(|e| LtboxError::Download(format!("Lenovo PTSTPD GET: {e}")))?;
    let env: Envelope = resp
        .body_mut()
        .read_json()
        .map_err(|e| LtboxError::Download(format!("Lenovo PTSTPD JSON: {e}")))?;
    if env.status_code != Some(200) {
        let msg = env.message.unwrap_or_else(|| "upstream error".to_string());
        return Err(LtboxError::Download(format!(
            "Lenovo PTSTPD status {:?}: {msg}",
            env.status_code
        )));
    }
    let data_value = env
        .data
        .as_ref()
        .ok_or_else(|| LtboxError::Download("Lenovo PTSTPD: missing data block".into()))?;
    let data = data_value
        .as_object()
        .ok_or_else(|| LtboxError::Download("Lenovo PTSTPD: data block is not an object".into()))?;
    let mut fields = Vec::with_capacity(FIELD_ORDER.len());
    for &key in FIELD_ORDER {
        let Some(val) = data.get(key) else { continue };
        let display = match val {
            serde_json::Value::Null => None,
            serde_json::Value::String(s) => Some(s.clone()),
            other => Some(other.to_string()),
        };
        fields.push((key.to_string(), display));
    }
    let data_pretty = serde_json::to_string_pretty(data_value).unwrap_or_default();
    Ok(MachineInfo {
        fields,
        data_pretty,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Field-order list must contain unique entries — duplicates would
    /// render the same key twice in the popup.
    #[test]
    fn field_order_has_no_duplicates() {
        let mut seen = std::collections::HashSet::new();
        for &k in FIELD_ORDER {
            assert!(seen.insert(k), "duplicate field key: {k}");
        }
    }
}
