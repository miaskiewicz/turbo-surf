//! Compiles src/browser_releases.json (the measured Chrome release registry) into
//! `const` Rust data, so the registry stays plain data that the re-version helper
//! (harness/detection/reversion-chrome.mjs) rewrites, while the crate keeps its
//! zero-cost `browser::RELEASES` / `browser::DEFAULT_MAJOR` constants.
use serde_json::Value;
use std::fmt::Write as _;

const SOURCE: &str = "src/browser_releases.json";

fn fail(message: impl std::fmt::Display) -> ! {
    panic!("{SOURCE}: {message}");
}

fn text<'a>(release: &'a Value, key: &str) -> &'a str {
    release[key]
        .as_str()
        .unwrap_or_else(|| fail(format!("{key} must be a string")))
}

fn strings(value: &Value, what: &str) -> Vec<String> {
    value
        .as_array()
        .unwrap_or_else(|| fail(format!("{what} must be an array")))
        .iter()
        .map(|item| {
            item.as_str()
                .unwrap_or_else(|| fail(format!("{what} entries must be strings")))
                .to_string()
        })
        .collect()
}

fn list(items: &[String]) -> String {
    format!(
        "&[{}]",
        items
            .iter()
            .map(|item| format!("{item:?}"))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

// Object of string → string, emitted in the JSON's (sorted) key order.
fn pairs(value: &Value, what: &str) -> String {
    let object = value
        .as_object()
        .unwrap_or_else(|| fail(format!("{what} must be an object")));
    let entries = object.iter().map(|(key, value)| {
        let value = value
            .as_str()
            .unwrap_or_else(|| fail(format!("{what}.{key} must be a string")));
        format!("({key:?}, {value:?})")
    });
    format!("&[{}]", entries.collect::<Vec<_>>().join(", "))
}

fn orders(value: &Value) -> String {
    let object = value
        .as_object()
        .unwrap_or_else(|| fail("requestOrders must be an object"));
    let entries = object.iter().map(|(class, order)| {
        format!(
            "({class:?}, {})",
            list(&strings(order, "requestOrders entry"))
        )
    });
    format!("&[{}]", entries.collect::<Vec<_>>().join(", "))
}

fn main() {
    println!("cargo:rerun-if-changed={SOURCE}");
    let registry: Value =
        serde_json::from_str(&std::fs::read_to_string(SOURCE).unwrap_or_else(|error| fail(error)))
            .unwrap_or_else(|error| fail(error));
    let default_major = registry["defaultMajor"]
        .as_u64()
        .unwrap_or_else(|| fail("defaultMajor must be an integer"));
    let releases = registry["releases"]
        .as_array()
        .filter(|releases| !releases.is_empty())
        .unwrap_or_else(|| fail("releases must be a non-empty array"));
    let mut previous = u64::MAX;
    let mut code = format!(
        "/// The reported version when no explicit major is chosen.\npub const DEFAULT_MAJOR: u16 = {default_major};\n\n/// Every configured Chrome release, newest first (generated from {SOURCE}).\npub const RELEASES: &[ChromeRelease] = &[\n"
    );
    for release in releases {
        let major = release["major"]
            .as_u64()
            .unwrap_or_else(|| fail("major must be an integer"));
        if major >= previous {
            fail("releases must be listed newest first without duplicates");
        }
        previous = major;
        let full_version = text(release, "fullVersion");
        if !full_version.starts_with(&format!("{major}.")) {
            fail(format!(
                "{major}: fullVersion {full_version} must start with the major"
            ));
        }
        let memory = release["defaultDeviceMemory"]
            .as_u64()
            .filter(|memory| *memory <= u64::from(u8::MAX))
            .unwrap_or_else(|| fail(format!("{major}: defaultDeviceMemory must be a u8")));
        let transport = release["transportBaseMajor"]
            .as_u64()
            .unwrap_or_else(|| fail(format!("{major}: transportBaseMajor must be an integer")));
        writeln!(
            code,
            "    ChromeRelease {{ major: {major}, full_version: {full_version:?}, header_order: {}, navigation_accept: {:?}, default_device_memory: {memory}, transport_base_major: {transport}, request_orders: {}, destination_accept: {}, destination_priority: {} }},",
            list(&strings(&release["headerOrder"], "headerOrder")),
            text(release, "navigationAccept"),
            orders(&release["requestOrders"]),
            pairs(&release["accept"], "accept"),
            pairs(&release["priority"], "priority"),
        )
        .unwrap();
    }
    code.push_str("];\n");
    if !releases
        .iter()
        .any(|release| release["major"].as_u64() == Some(default_major))
    {
        fail(format!("defaultMajor {default_major} has no release entry"));
    }
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    std::fs::write(out.join("browser_releases.rs"), code).unwrap();
}
