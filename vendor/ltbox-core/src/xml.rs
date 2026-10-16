//! Bounded parsing of external firmware XML before any device operation.
use std::{
    fs::File,
    io::{self, Read},
    path::Path,
};

// Firmware manifests are text metadata, not partition payloads. Keep generous
// limits while bounding both input allocation and the DOM's node amplification.
pub const MAX_XML_BYTES: usize = 64 * 1024 * 1024;
const MAX_XML_NODES: u32 = 1_000_000;

/// Read at most the supported XML size, including files that grow while read.
pub fn read(path: &Path) -> io::Result<String> {
    read_bounded(File::open(path)?, MAX_XML_BYTES)
}

fn read_bounded(reader: impl Read, limit: usize) -> io::Result<String> {
    let mut text = String::new();
    reader.take(limit as u64 + 1).read_to_string(&mut text)?;
    if text.len() > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "XML input exceeds size limit",
        ));
    }
    Ok(text)
}

/// Parse without DTD/entity loading, with byte and DOM-node limits.
pub fn parse(text: &str) -> io::Result<roxmltree::Document<'_>> {
    parse_bounded(text, MAX_XML_BYTES, MAX_XML_NODES)
}

fn parse_bounded(text: &str, bytes: usize, nodes: u32) -> io::Result<roxmltree::Document<'_>> {
    if text.len() > bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "XML input exceeds size limit",
        ));
    }
    roxmltree::Document::parse_with_options(
        text,
        roxmltree::ParsingOptions {
            nodes_limit: nodes,
            ..Default::default()
        },
    )
    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn input_limit_checks_bytes_and_accepts_exact_boundary() {
        assert_eq!(read_bounded("<a/>".as_bytes(), 4).unwrap(), "<a/>");
        assert!(read_bounded("<a/> ".as_bytes(), 4).is_err());
        assert!(parse_bounded("<a/> ", 4, 10).is_err());
    }
    #[test]
    fn node_limit_and_dtd_are_rejected() {
        assert!(parse_bounded("<a><b/><c/></a>", 100, 2).is_err());
        assert!(parse("<!DOCTYPE a [<!ENTITY x 'text'>]><a>&x;</a>").is_err());
        assert!(parse("<a><b id='1'/></a>").is_ok());
    }
}
