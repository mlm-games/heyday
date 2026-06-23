use flate2::read::GzDecoder;
use quick_xml::Reader as XmlReader;
use quick_xml::events::Event;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read};

#[derive(Clone, Debug, Default)]
pub struct AppstreamMeta {
    pub name: String,
    pub summary: String,
    pub description: String,
    pub version: Option<String>,
    pub license: Option<String>,
    pub developer: Option<String>,
    pub homepage: Option<String>,
}

/// Parse uncompressed appstream XML.
pub fn parse_appstream_xml(reader: impl Read) -> HashMap<String, AppstreamMeta> {
    parse_appstream_buf(BufReader::new(reader))
}

/// Parse gzip-compressed appstream XML.
pub fn parse_appstream_xml_gz(reader: impl Read) -> HashMap<String, AppstreamMeta> {
    parse_appstream_buf(BufReader::new(GzDecoder::new(reader)))
}

fn parse_appstream_buf<R: BufRead>(reader: R) -> HashMap<String, AppstreamMeta> {
    let mut map = HashMap::new();
    let mut xml = XmlReader::from_reader(reader);
    xml.config_mut().trim_text(true);

    let mut buf = Vec::new();
    let mut in_component = false;
    let mut meta = AppstreamMeta::default();
    let mut id = String::new();
    let mut current_tag = String::new();
    let mut in_desc = false;
    let mut in_url = false;
    let mut url_type = String::new();
    let mut text_buf = String::new();
    let mut in_releases = false;
    let mut in_developer = false;

    loop {
        match xml.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if tag == "component" {
                    in_component = true;
                    meta = AppstreamMeta::default();
                    id.clear();
                } else if in_component {
                    let has_lang = e
                        .attributes()
                        .any(|a| a.ok().is_some_and(|a| a.key.as_ref() == b"xml:lang"));
                    if !has_lang {
                        match tag.as_str() {
                            "description" => in_desc = true,
                            "url" => {
                                in_url = true;
                                url_type = e
                                    .attributes()
                                    .filter_map(|a| a.ok())
                                    .find(|a| a.key.as_ref() == b"type")
                                    .and_then(|a| String::from_utf8(a.value.to_vec()).ok())
                                    .unwrap_or_default();
                            }
                            "releases" => in_releases = true,
                            "release" if in_releases && meta.version.is_none() => {
                                meta.version = e
                                    .attributes()
                                    .filter_map(|a| a.ok())
                                    .find(|a| a.key.as_ref() == b"version")
                                    .and_then(|a| String::from_utf8(a.value.to_vec()).ok());
                            }
                            "developer" => in_developer = true,
                            _ => {}
                        }
                        current_tag = tag;
                    } else {
                        current_tag.clear();
                    }
                }
            }
            Ok(Event::Empty(ref e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if in_component && tag == "release" && in_releases && meta.version.is_none() {
                    meta.version = e
                        .attributes()
                        .filter_map(|a| a.ok())
                        .find(|a| a.key.as_ref() == b"version")
                        .and_then(|a| String::from_utf8(a.value.to_vec()).ok());
                }
            }
            Ok(Event::End(ref e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if tag == "component" {
                    in_component = false;
                    if !id.is_empty() {
                        meta.description = text_buf.trim().to_string();
                        text_buf.clear();
                        map.insert(id.clone(), meta.clone());
                    }
                } else if in_component {
                    match tag.as_str() {
                        "description" => in_desc = false,
                        "url" => {
                            in_url = false;
                            url_type.clear();
                        }
                        "releases" => in_releases = false,
                        "developer" => in_developer = false,
                        _ => {}
                    }
                }
            }
            Ok(Event::Text(ref e)) => {
                if in_component {
                    let txt = String::from_utf8_lossy(e.as_ref()).to_string();
                    if in_desc {
                        text_buf.push_str(&txt);
                        text_buf.push(' ');
                    } else if in_developer {
                        meta.developer = Some(txt.trim().to_string());
                    } else if !current_tag.is_empty() {
                        let trimmed = txt.trim().to_string();
                        match current_tag.as_str() {
                            "id" => id = txt,
                            "name" => meta.name = trimmed.clone(),
                            "summary" => meta.summary = trimmed.clone(),
                            "project_license" => meta.license = Some(trimmed.clone()),
                            "developer_name" => meta.developer = Some(trimmed.clone()),
                            _ => {}
                        }
                        if in_url && url_type == "homepage" {
                            meta.homepage = Some(trimmed);
                            in_url = false;
                        }
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                log::warn!("appstream XML parse error: {e}");
                break;
            }
            _ => {}
        }
        buf.clear();
    }

    map
}
