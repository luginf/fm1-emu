//! Download the catalog's firmwares, with no Node.js: what `tools/fetch_firmwares.mjs` does,
//! from the same catalog (`web/firmwares.json`, compiled into the binary) and the same
//! unpacking as the web page (`web/fwsc.js`; here `fm1_soc::fwsc`). For each firmware, into
//! the firmware directory (`reference/firmwares` by default):
//!   <id>.fwsc      the package as published
//!   <id>.app.bin   its app
//!   <id>.xip.bin   the app behind 0x120 bytes of padding, loaded at 0x02000120 (`--fw ID`)
//!
//! A package is fetched the way the page does it (`fetchPackage` in web/fwsc.js): the entry's
//! `package` URL, or when that has gone (a newer version replaced it) the one its `manifest`
//! names, or the .fwsc its `installer` page links now (in the page or the scripts it loads).
//! An entry with `embedded` keeps its package in a base64 variable of its installer page.

use serde_json::Value;
use std::io::Read;
use std::path::Path;
use std::time::Duration;

const CATALOG: &str = include_str!("../../../web/firmwares.json");

#[derive(Clone, Debug)]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub version: String,
    /// bring your own (M-VAVE's stock firmware: `--fwsc FM-1.fwsc`)
    pub byo: bool,
    pub package: Option<String>,
    pub installer: Option<String>,
    pub manifest: Option<String>,
    pub embedded: Option<String>,
    /// where a bring-your-own package comes from (M-VAVE's own server)
    pub download: Option<String>,
}

/// The catalog compiled into the binary.
pub fn catalog() -> Vec<Entry> {
    let v: Value = serde_json::from_str(CATALOG).expect("web/firmwares.json is valid JSON");
    let s = |f: &Value, k: &str| f.get(k).and_then(Value::as_str).map(str::to_string);
    v["firmwares"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|f| Entry {
                    id: s(f, "id").unwrap_or_default(),
                    name: s(f, "name").unwrap_or_default(),
                    version: s(f, "version").unwrap_or_default(),
                    byo: f.get("byo").and_then(Value::as_bool).unwrap_or(false),
                    package: s(f, "package"),
                    installer: s(f, "installer"),
                    manifest: s(f, "manifest"),
                    embedded: s(f, "embedded"),
                    download: s(f, "download"),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(30))
        .user_agent(concat!("fm1-live/", env!("CARGO_PKG_VERSION")))
        .build()
}

fn get_bytes(url: &str) -> Result<Vec<u8>, String> {
    let r = agent().get(url).call().map_err(|e| match e {
        ureq::Error::Status(code, _) => format!("{url}: {code}"),
        e => format!("{url}: {e}"),
    })?;
    let mut v = Vec::new();
    r.into_reader().take(64 << 20).read_to_end(&mut v).map_err(|e| format!("{url}: {e}"))?;
    Ok(v)
}

fn get_text(url: &str) -> Result<String, String> {
    Ok(String::from_utf8_lossy(&get_bytes(url)?).into_owned())
}

fn join(base: &str, rel: &str) -> Result<String, String> {
    let b = url::Url::parse(base).map_err(|e| format!("{base}: {e}"))?;
    b.join(rel).map(|u| u.to_string()).map_err(|e| format!("{base} + {rel}: {e}"))
}

/// Every `name.fwsc` in a text (a path or URL: word characters, `.`, `/`, `-`), but the stock
/// firmware's FM-1.fwsc that installer pages also mention.
fn fwsc_names(text: &str) -> Vec<String> {
    let ok = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '/' | '-');
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(at) = text[from..].find(".fwsc") {
        let end = from + at + 5;
        let start = text[..from + at].char_indices().rev().take_while(|&(_, c)| ok(c)).last().map_or(from + at, |(i, _)| i);
        let name = &text[start..end];
        let base = name.rsplit('/').next().unwrap_or(name);
        if start < end && name.len() > 5 && !base.eq_ignore_ascii_case("FM-1.fwsc") {
            out.push(name.to_string());
        }
        from = end;
    }
    out
}

/// The `src="...js"` scripts of a page.
fn script_srcs(page: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(at) = page[from..].find("src=\"") {
        let s = from + at + 5;
        let Some(len) = page[s..].find('"') else { break };
        let src = &page[s..s + len];
        if src.ends_with(".js") || src.ends_with(".mjs") {
            out.push(src.to_string());
        }
        from = s + len;
    }
    out
}

fn base64(s: &str) -> Option<Vec<u8>> {
    let (mut out, mut acc, mut bits) = (Vec::new(), 0u32, 0);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            _ => return None,
        } as u32;
        acc = acc << 6 | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// The text of `var = "BASE64"` (or with single quotes) in a page: the embedded package.
fn embedded_package(page: &str, var: &str) -> Option<Vec<u8>> {
    let mut from = 0;
    while let Some(at) = page[from..].find(var) {
        from += at + var.len();
        let Some(r) = page[from..].trim_start().strip_prefix('=') else { continue };
        let r = r.trim_start();
        let Some(q) = r.chars().next().filter(|c| matches!(c, '"' | '\'')) else { continue };
        let body = &r[1..];
        let Some(end) = body.find(q) else { continue };
        let b64 = &body[..end];
        if !b64.is_empty() && b64.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'+' | b'/' | b'=')) {
            return base64(b64);
        }
    }
    None
}

/// M-VAVE's own package (a `byo` entry): `<dir>/<id>.fwsc`, else an `FM-1.fwsc` in `dir` or in
/// the current directory, else downloaded from the vendor's server into `<dir>/<id>.fwsc`
/// (a browser cannot do that, its server does not allow other pages; this program can).
pub fn stock_package(e: &Entry, dir: &Path) -> Result<std::path::PathBuf, String> {
    let own = dir.join(format!("{}.fwsc", e.id));
    for p in [own.clone(), dir.join("FM-1.fwsc"), Path::new("FM-1.fwsc").to_path_buf()] {
        if p.is_file() {
            return Ok(p);
        }
    }
    let url = e.download.as_deref().ok_or("no download URL in the catalog")?;
    eprintln!("{}: downloading M-VAVE's package from its server ({url})", e.id);
    let bytes = get_bytes(url)?;
    fm1_soc::fwsc::unpack(&bytes).map_err(|m| format!("{url}: not an FM-1 package ({m})"))?;
    std::fs::create_dir_all(dir).map_err(|err| format!("{}: {err}", dir.display()))?;
    std::fs::write(&own, &bytes).map_err(|err| format!("{}: {err}", own.display()))?;
    Ok(own)
}

/// The package of a catalog entry and the URL it came from.
pub fn fetch_package(e: &Entry) -> Result<(Vec<u8>, String), String> {
    if let Some(var) = &e.embedded {
        let page_url = e.installer.as_deref().ok_or("an embedded package needs an installer page")?;
        let page = get_text(page_url)?;
        let bytes = embedded_package(&page, var).ok_or_else(|| format!("{page_url} has no {var}"))?;
        return Ok((bytes, page_url.to_string()));
    }
    let package = e.package.as_deref().ok_or("no package URL in the catalog")?;
    let mut last = match get_bytes(package) {
        Ok(b) => return Ok((b, package.to_string())),
        Err(e) => e,
    };
    if let Some(manifest) = &e.manifest {
        let m: Value = serde_json::from_str(&get_text(manifest)?).map_err(|e| format!("{manifest}: {e}"))?;
        if let Some(name) = m.get("package").or_else(|| m.get("file")).and_then(Value::as_str) {
            let url = join(manifest, name)?;
            match get_bytes(&url) {
                Ok(b) => return Ok((b, url)),
                Err(e) => last = e,
            }
        }
    }
    let Some(installer) = &e.installer else { return Err(last) };
    let page = get_text(installer)?;
    let (mut names, mut base) = (fwsc_names(&page), installer.clone());
    if names.is_empty() {
        for src in script_srcs(&page) {
            let url = join(installer, &src)?;
            names = fwsc_names(&get_text(&url)?);
            if !names.is_empty() {
                base = url;
                break;
            }
        }
    }
    for n in names {
        let url = join(&base, &n)?;
        match get_bytes(&url) {
            Ok(b) => return Ok((b, url)),
            Err(e) => last = e,
        }
    }
    Err(format!("{last}, and {installer} names no package that loads"))
}

/// The app inside a package: `app.bin` of its app area, less the slot's 0xFF padding.
pub fn app_from_package(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let u = fm1_soc::fwsc::unpack(bytes)?;
    let h = u.dir.iter().find(|h| h.name == "app.bin" && h.flags == 0x82).ok_or("no app.bin in the app area")?;
    let app = u.xip.get(h.offset..h.offset + h.size).ok_or("app.bin runs past the app area")?;
    let end = app.iter().rposition(|&b| b != 0xff).map_or(0, |i| i + 1);
    Ok(app[..end].to_vec())
}

/// Fetch one firmware into `dir` (created for the occasion); the message names its source.
pub fn fetch_into(e: &Entry, dir: &Path) -> Result<String, String> {
    std::fs::create_dir_all(dir).map_err(|err| format!("{}: {err}", dir.display()))?;
    let (bytes, url) = fetch_package(e)?;
    let app = app_from_package(&bytes)?;
    let mut xip = vec![0u8; 0x120];
    xip.extend_from_slice(&app);
    let out = |ext: &str, data: &[u8]| std::fs::write(dir.join(format!("{}.{ext}", e.id)), data).map_err(|err| format!("{}.{ext}: {err}", e.id));
    out("fwsc", &bytes)?;
    out("app.bin", &app)?;
    out("xip.bin", &xip)?;
    Ok(format!("{} ({} bytes)", url.trim_end_matches('/').rsplit('/').next().unwrap_or(&url), app.len()))
}

/// `fm1-live --fetch [ID ...]`: the listed firmwares, or all of the catalog's. Returns the
/// number of failures.
pub fn run(ids: &[String], dir: &Path) -> usize {
    let cat = catalog();
    for id in ids {
        if !cat.iter().any(|e| &e.id == id && !e.byo) {
            eprintln!("{id}: not in the catalog (fm1-live --fw list)");
            return 1;
        }
    }
    let mut failed = 0;
    for e in cat.iter().filter(|e| !e.byo && (ids.is_empty() || ids.contains(&e.id))) {
        match fetch_into(e, dir) {
            Ok(m) => println!("{:13} {m}", e.id),
            Err(m) => {
                failed += 1;
                println!("{:13} FAILED {m}", e.id);
            }
        }
    }
    println!("-> {}", dir.display());
    failed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_in_a_page() {
        let page = r#"fetch("firmware/felucca-1.7.fwsc"); other "FM-1.fwsc" and /x/FM-1.fwsc, "https://a.b/c/d-2.0.fwsc""#;
        assert_eq!(fwsc_names(page), vec!["firmware/felucca-1.7.fwsc", "//a.b/c/d-2.0.fwsc"]);
    }

    #[test]
    fn scripts_of_a_page() {
        assert_eq!(script_srcs(r#"<script src="app.js"></script><img src="a.png"><script src="/m/x.mjs">"#), vec!["app.js", "/m/x.mjs"]);
    }

    #[test]
    fn embedded_base64() {
        let page = "const DEFAULT_FIRMWARE = 'aGVsbG8=';";
        assert_eq!(embedded_package(page, "DEFAULT_FIRMWARE").unwrap(), b"hello");
        assert!(embedded_package("var X = 1", "DEFAULT_FIRMWARE").is_none());
    }

    #[test]
    fn catalog_has_the_firmwares() {
        let c = catalog();
        assert!(c.iter().any(|e| e.id == "felucca" && e.package.is_some()));
        assert!(c.iter().any(|e| e.id == "mvave" && e.byo));
    }
}
