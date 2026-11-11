use std::rc::Rc;

const MAX_ENTRIES: usize = 8;
const MAX_HTML_BYTES: usize = 4 * 1024 * 1024;

/// A bounded cache of immutable snapshots. Exact HTML keys invalidate changed pages.
pub(crate) struct ParseCache<T> {
    entries: Vec<(String, Rc<T>)>,
}

impl<T> ParseCache<T> {
    pub(crate) const fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub(crate) fn get_or_insert_with(
        &mut self,
        html: &str,
        parse: impl FnOnce(&str) -> T,
    ) -> Rc<T> {
        if let Some(index) = self.entries.iter().position(|(key, _)| key == html) {
            let entry = self.entries.remove(index);
            let tree = entry.1.clone();
            self.entries.push(entry);
            return tree;
        }
        let tree = Rc::new(parse(html));
        if html.len() <= MAX_HTML_BYTES {
            while self.entries.len() >= MAX_ENTRIES
                || self.entries.iter().map(|(key, _)| key.len()).sum::<usize>() + html.len()
                    > MAX_HTML_BYTES
            {
                self.entries.remove(0);
            }
            self.entries.push((html.to_string(), tree.clone()));
        }
        tree
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alternating_snapshots_retain_both_parses_and_invalidate_changed_html() {
        let mut cache = ParseCache::new();
        let left = cache.get_or_insert_with("left", str::to_string);
        let right = cache.get_or_insert_with("right", str::to_string);
        assert!(Rc::ptr_eq(
            &left,
            &cache.get_or_insert_with("left", |_| panic!("reparsed left"))
        ));
        assert!(Rc::ptr_eq(
            &right,
            &cache.get_or_insert_with("right", |_| panic!("reparsed right"))
        ));
        let changed = cache.get_or_insert_with("left changed", str::to_string);
        assert_eq!(&*changed, "left changed");
        assert!(!Rc::ptr_eq(&left, &changed));
    }
    #[test]
    fn cache_evicts_least_recently_used_and_bounds_input_bytes() {
        let mut cache = ParseCache::new();
        let first = cache.get_or_insert_with("0", str::to_string);
        for i in 1..MAX_ENTRIES {
            cache.get_or_insert_with(&i.to_string(), str::to_string);
        }
        cache.get_or_insert_with("0", |_| panic!("oldest should still be cached"));
        cache.get_or_insert_with("new", str::to_string);
        assert!(Rc::ptr_eq(
            &first,
            &cache.get_or_insert_with("0", str::to_string)
        ));
        assert!(!cache.entries.iter().any(|(key, _)| key == "1"));
        cache.get_or_insert_with(&"x".repeat(MAX_HTML_BYTES), str::to_string);
        assert_eq!(cache.entries.len(), 1);
        cache.get_or_insert_with(&"y".repeat(MAX_HTML_BYTES + 1), str::to_string);
        assert_eq!(
            cache.entries.len(),
            1,
            "oversized snapshots bypass retention"
        );
    }
}
