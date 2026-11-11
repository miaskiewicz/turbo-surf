use super::{BrowserStorage, StorageEntries, StorageKey};
use serde::{Deserialize, Serialize};

// Code units keep isolated UTF-16 surrogates intact across the JSON transport.
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct OriginState {
    origin: String,
    local_storage: Vec<StorageItem>,
}

#[derive(Deserialize, Serialize)]
struct StorageItem {
    name: Vec<u16>,
    value: Vec<u16>,
}

fn origin_key(origin: &str) -> Result<StorageKey, String> {
    let url = url::Url::parse(origin).map_err(|error| error.to_string())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("Storage state requires an HTTP or HTTPS origin".into());
    }
    let origin = url.origin().ascii_serialization();
    let site = turbo_surf_core::request_context::schemeful_site(&origin)
        .ok_or("Storage state origin has no schemeful site")?;
    Ok(StorageKey {
        partition: 0,
        site,
        origin,
        cross_site_ancestor: false,
        session: false,
        tab: 0,
    })
}

fn prepare(origin: OriginState) -> Result<(StorageKey, StorageEntries), String> {
    let key = origin_key(&origin.origin)?;
    let mut positions = std::collections::HashMap::new();
    let mut values: StorageEntries = Vec::new();
    for item in origin.local_storage {
        if let Some(index) = positions.get(&item.name).copied() {
            values[index] = (item.name, item.value);
        } else {
            positions.insert(item.name.clone(), values.len());
            values.push((item.name, item.value));
        }
    }
    if values
        .iter()
        .map(|(name, value)| name.len() + value.len())
        .sum::<usize>()
        > 5 * 1024 * 1024
    {
        return Err("Storage quota exceeded".into());
    }
    Ok((key, values))
}

impl BrowserStorage {
    /// Import normal top-level local storage before constructing page realms.
    /// The host transport uses UTF-16 arrays, rather than lossy Rust strings.
    pub fn import_local_storage_state(&self, json: &str) -> Result<(), String> {
        let origins: Vec<OriginState> =
            serde_json::from_str(json).map_err(|error| error.to_string())?;
        let prepared = origins
            .into_iter()
            .map(prepare)
            .collect::<Result<Vec<_>, _>>()?;
        let mut areas = self.0.lock().map_err(|_| "Storage backing unavailable")?;
        let additional = prepared
            .iter()
            .filter(|(key, _)| !areas.by_origin.contains_key(key))
            .count();
        let additional = u32::try_from(additional).map_err(|_| "Too many storage areas")?;
        areas
            .next
            .checked_add(additional)
            .ok_or("Too many storage areas")?;
        for (key, values) in prepared {
            let id = match areas.by_origin.get(&key).copied() {
                Some(id) => id,
                None => {
                    areas.next += 1;
                    let id = areas.next;
                    areas.by_origin.insert(key, id);
                    id
                }
            };
            areas.values.insert(id, values);
        }
        Ok(())
    }

    /// Export the context's normal local storage without credentialless,
    /// cross-site partitions or tab-specific session storage.
    pub fn local_storage_state(&self) -> Result<String, String> {
        let areas = self.0.lock().map_err(|_| "Storage backing unavailable")?;
        let mut origins = Vec::new();
        for (key, id) in &areas.by_origin {
            if origin_key(&key.origin).as_ref() != Ok(key) {
                continue;
            }
            let Some(values) = areas.values.get(id).filter(|values| !values.is_empty()) else {
                continue;
            };
            origins.push(OriginState {
                origin: key.origin.clone(),
                local_storage: values
                    .iter()
                    .map(|(name, value)| StorageItem {
                        name: name.clone(),
                        value: value.clone(),
                    })
                    .collect(),
            });
        }
        serde_json::to_string(&origins).map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_roundtrips_utf16_and_is_shared_by_tabs() {
        let storage = BrowserStorage::default();
        let input = r#"[{"origin":"https://example.test","localStorage":[{"name":[55296],"value":[57343]},{"name":[97],"value":[49]},{"name":[97],"value":[50]}]}]"#;
        storage.import_local_storage_state(input).unwrap();
        let snapshot: serde_json::Value =
            serde_json::from_str(&storage.local_storage_state().unwrap()).unwrap();
        assert_eq!(
            snapshot[0]["localStorage"][0]["name"],
            serde_json::json!([55296])
        );
        assert_eq!(
            snapshot[0]["localStorage"][0]["value"],
            serde_json::json!([57343])
        );
        assert_eq!(
            snapshot[0]["localStorage"][1]["value"],
            serde_json::json!([50])
        );
        assert_eq!(
            storage.new_tab().unwrap().local_storage_state().unwrap(),
            storage.local_storage_state().unwrap()
        );
    }

    #[test]
    fn invalid_import_is_atomic_and_private_storage_is_excluded() {
        let storage = BrowserStorage::default();
        storage.import_local_storage_state(r#"[{"origin":"https://example.test","localStorage":[{"name":[97],"value":[49]}]}]"#).unwrap();
        let original = storage.local_storage_state().unwrap();
        assert!(storage.import_local_storage_state(r#"[{"origin":"https://example.test","localStorage":[]},{"origin":"data:text/html,","localStorage":[]}]"#).is_err());
        assert_eq!(storage.local_storage_state().unwrap(), original);
        assert!(storage.import_local_storage_state("not-json").is_err());
        {
            let mut areas = storage.0.lock().unwrap();
            for (id, session, partition) in [(100, true, 0), (101, false, 1)] {
                let mut key = origin_key("https://example.test").unwrap();
                key.session = session;
                key.partition = partition;
                areas.by_origin.insert(key, id);
                areas.values.insert(id, vec![(vec![98], vec![50])]);
            }
        }
        assert_eq!(storage.local_storage_state().unwrap(), original);
    }

    #[test]
    fn import_obeys_native_storage_quota_and_area_id_bounds() {
        assert!(prepare(OriginState {
            origin: "https://example.test".into(),
            local_storage: vec![StorageItem {
                name: vec![1],
                value: vec![2; 5 * 1024 * 1024]
            }],
        })
        .is_err());
        let storage = BrowserStorage::default();
        storage.0.lock().unwrap().next = u32::MAX;
        assert!(storage
            .import_local_storage_state(r#"[{"origin":"https://example.test","localStorage":[]}]"#)
            .is_err());
        assert_eq!(storage.local_storage_state().unwrap(), "[]");
    }
}
