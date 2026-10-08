//! The web services: JLCPCB's parts search, and EasyEDA's part data and 3D models.

use serde_json::Value;
use std::time::Duration;

const SEARCH_URL: &str = "https://jlcpcb.com/api/overseas-pcb-order/v1/shoppingCart/smtGood/selectSmtComponentList";
const COMPONENT_URL: &str = "https://easyeda.com/api/products/{lcsc}/components?version=6.4.19.5";
const OBJ_URL: &str = "https://modules.easyeda.com/3dmodel/{uuid}";
const STEP_URL: &str = "https://modules.easyeda.com/qAxj6KHrDKw4blvCG8QJPs7Y/{uuid}";

/// 3D models can be large; anything past this isn't a part model.
const MAX_MODEL: u64 = 200 << 20;

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(60)))
        .user_agent(concat!("cadrs/", env!("CARGO_PKG_VERSION"), " (+https://github.com/rvdende/cadrs)"))
        .build()
        .into()
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn easyeda_get(url: &str) -> Result<ureq::http::Response<ureq::Body>, String> {
    agent().get(url).header("Referer", "https://easyeda.com/").header("Origin", "https://easyeda.com").call().map_err(err)
}

/// A part in JLCPCB's catalogue (what the search lists).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Hit {
    /// The LCSC part number ("C2764087"), the key everything else is fetched by.
    pub lcsc: String,
    /// Manufacturer part number.
    pub mpn: String,
    pub manufacturer: String,
    pub package: String,
    pub description: String,
    pub category: String,
    pub stock: u64,
    /// A JLCPCB basic part (no extra loading fee) rather than an extended one.
    pub basic: bool,
    /// The unit price at the smallest quantity, USD.
    pub price: Option<f64>,
    pub datasheet: String,
}

fn hit(v: &Value) -> Option<Hit> {
    let s = |k: &str| crate::convert::latin(v[k].as_str().unwrap_or_default());
    Some(Hit {
        lcsc: v["componentCode"].as_str()?.to_string(),
        mpn: s("componentModelEn"),
        manufacturer: s("componentBrandEn"),
        package: s("componentSpecificationEn"),
        description: s("describe"),
        category: s("componentTypeEn"),
        stock: v["stockCount"].as_u64().unwrap_or(0),
        basic: v["componentLibraryType"].as_str() == Some("base"),
        price: v["componentPrices"].as_array().and_then(|p| p.first()).and_then(|p| p["productPrice"].as_f64()),
        datasheet: s("dataManualUrl"),
    })
}

/// Searches JLCPCB's parts (words, a part number or an LCSC number), `page` from 1, 25 to a
/// page. Returns the page's parts and how many match in all.
pub fn search(keyword: &str, page: u32) -> Result<(Vec<Hit>, u64), String> {
    let body = serde_json::json!({ "keyword": keyword, "currentPage": page.max(1), "pageSize": 25 }).to_string();
    let mut r = agent().post(SEARCH_URL).header("Content-Type", "application/json").send(body).map_err(err)?;
    let v: Value = serde_json::from_str(&r.body_mut().read_to_string().map_err(err)?).map_err(err)?;
    parse_search(&v)
}

/// A search response's parts and total.
pub fn parse_search(v: &Value) -> Result<(Vec<Hit>, u64), String> {
    if v["code"].as_i64() != Some(200) {
        return Err(format!("search failed: {}", v["message"].as_str().unwrap_or("no reason given")));
    }
    let info = &v["data"]["componentPageInfo"];
    let hits = info["list"].as_array().map(|l| l.iter().filter_map(hit).collect()).unwrap_or_default();
    Ok((hits, info["total"].as_u64().unwrap_or(0)))
}

/// A part's EasyEDA data (symbol, footprint and 3D model reference): the API's `result`.
pub fn component(lcsc: &str) -> Result<Value, String> {
    let mut r = easyeda_get(&COMPONENT_URL.replace("{lcsc}", lcsc))?;
    let v: Value = serde_json::from_str(&r.body_mut().read_to_string().map_err(err)?).map_err(err)?;
    if v["success"].as_bool() != Some(true) || v["result"].is_null() {
        return Err(format!("{lcsc}: EasyEDA has no symbol or footprint for this part"));
    }
    Ok(v["result"].clone())
}

/// A 3D model as Wavefront OBJ (mm), for its extent.
pub fn model_obj(uuid: &str) -> Result<String, String> {
    easyeda_get(&OBJ_URL.replace("{uuid}", uuid))?.body_mut().with_config().limit(MAX_MODEL).read_to_string().map_err(err)
}

/// A 3D model as STEP.
pub fn model_step(uuid: &str) -> Result<Vec<u8>, String> {
    let bytes = easyeda_get(&STEP_URL.replace("{uuid}", uuid))?.body_mut().with_config().limit(MAX_MODEL).read_to_vec().map_err(err)?;
    if !bytes.starts_with(b"ISO-10303-21") {
        return Err("the STEP model isn't a STEP file".into());
    }
    Ok(bytes)
}
