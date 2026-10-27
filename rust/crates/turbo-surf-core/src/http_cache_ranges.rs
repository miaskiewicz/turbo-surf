//! Bounded sparse byte ranges for a single strongly validated representation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ByteRange {
    pub start: usize,
    pub end: usize,
}
impl ByteRange {
    pub fn request(value: &str, total: usize) -> Option<Self> {
        let (start, end) = value.strip_prefix("bytes=")?.split_once('-')?;
        let suffix = start.is_empty();
        let start = if suffix {
            total.saturating_sub(end.parse::<usize>().ok()?)
        } else {
            start.parse().ok()?
        };
        let end = if suffix || end.is_empty() {
            total
        } else {
            end.parse::<usize>().ok()?.checked_add(1)?.min(total)
        };
        (start < end && start < total).then_some(Self { start, end })
    }
    pub fn content(value: &str) -> Option<(Self, usize)> {
        let (range, total) = value.strip_prefix("bytes ")?.split_once('/')?;
        let (start, end) = range.split_once('-')?;
        let total = total.parse().ok()?;
        let start = start.parse().ok()?;
        let end = end.parse::<usize>().ok()?.checked_add(1)?;
        (start < end && end <= total).then_some((Self { start, end }, total))
    }
    pub fn header(self, total: usize) -> String {
        format!("bytes {}-{}/{total}", self.start, self.end - 1)
    }
}
#[derive(Clone)]
pub(crate) struct SparseBody {
    pub total: usize,
    parts: Vec<(usize, Vec<u8>)>,
}
impl SparseBody {
    pub fn new(total: usize) -> Self {
        Self {
            total,
            parts: Vec::new(),
        }
    }
    pub fn insert(&mut self, range: ByteRange, body: Vec<u8>) -> bool {
        if body.len() != range.end - range.start
            || range.end > self.total
            || self.parts.len() >= 1024
        {
            return false;
        }
        self.parts.push((range.start, body));
        self.parts.sort_by_key(|(start, _)| *start);
        true
    }
    pub fn append(&mut self, other: Self) {
        if self.parts.len() + other.parts.len() > 1024 {
            self.parts.clear();
        }
        self.parts.extend(other.parts);
        self.parts.sort_by_key(|(start, _)| *start);
    }
    pub fn size(&self) -> usize {
        self.parts.iter().map(|(_, bytes)| bytes.len()).sum()
    }
    pub fn prefix(&self, range: ByteRange) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut current = range.start;
        while current < range.end {
            let Some((start, part)) = self
                .parts
                .iter()
                .filter(|(start, bytes)| *start <= current && *start + bytes.len() > current)
                .max_by_key(|(start, bytes)| *start + bytes.len())
            else {
                break;
            };
            let end = (start + part.len()).min(range.end);
            bytes.extend_from_slice(&part[current - start..end - start]);
            current = end;
        }
        bytes
    }
    pub fn slice(&self, range: ByteRange) -> Option<Vec<u8>> {
        let bytes = self.prefix(range);
        (bytes.len() == range.end - range.start).then_some(bytes)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_sparse_ranges_cover_only_received_bytes() {
        let mut body = SparseBody::new(10);
        assert!(body.insert(ByteRange { start: 0, end: 4 }, b"abcd".to_vec()));
        assert_eq!(body.slice(ByteRange { start: 2, end: 6 }), None);
        assert_eq!(body.prefix(ByteRange { start: 2, end: 6 }), b"cd");
        assert!(body.insert(ByteRange { start: 4, end: 10 }, b"efghij".to_vec()));
        assert_eq!(
            body.slice(ByteRange { start: 0, end: 10 }).unwrap(),
            b"abcdefghij"
        );
        assert!(!body.insert(ByteRange { start: 0, end: 10 }, b"bad".to_vec()));
    }
    #[test]
    fn range_syntax_rejects_overflow_empty_and_multiple_ranges() {
        assert_eq!(
            ByteRange::request("bytes=-4", 10),
            Some(ByteRange { start: 6, end: 10 })
        );
        assert_eq!(
            ByteRange::request("bytes=5-", 10),
            Some(ByteRange { start: 5, end: 10 })
        );
        assert_eq!(
            ByteRange::request("bytes=1-100", 10),
            Some(ByteRange { start: 1, end: 10 })
        );
        assert_eq!(ByteRange::request("bytes=0-1,4-5", 10), None);
        assert_eq!(ByteRange::request("bytes=10-20", 10), None);
        assert_eq!(
            ByteRange::content("bytes 4-9/10"),
            Some((ByteRange { start: 4, end: 10 }, 10))
        );
        assert_eq!(ByteRange::content("bytes 4-10/10"), None);
        assert_eq!(ByteRange::content("bytes 4-9/*"), None);
    }
    #[test]
    fn appending_past_the_part_limit_keeps_only_the_new_parts() {
        let mut many = SparseBody::new(4096);
        for i in 0..1024 {
            assert!(many.insert(
                ByteRange {
                    start: i,
                    end: i + 1
                },
                vec![b'a']
            ));
        }
        assert!(!many.insert(
            ByteRange {
                start: 2000,
                end: 2001
            },
            vec![b'z']
        ));
        let mut more = SparseBody::new(4096);
        assert!(more.insert(
            ByteRange {
                start: 3000,
                end: 3002
            },
            b"xy".to_vec()
        ));
        many.append(more);
        assert_eq!(many.size(), 2);
        assert_eq!(
            many.slice(ByteRange {
                start: 3000,
                end: 3002
            })
            .unwrap(),
            b"xy"
        );
        assert_eq!(many.slice(ByteRange { start: 0, end: 1 }), None);
        // Exactly 1024 parts after the append is still within the limit.
        let mut full = SparseBody::new(4096);
        for i in 0..1023 {
            assert!(full.insert(
                ByteRange {
                    start: i,
                    end: i + 1
                },
                vec![b'a']
            ));
        }
        let mut last = SparseBody::new(4096);
        assert!(last.insert(
            ByteRange {
                start: 1023,
                end: 1024
            },
            vec![b'b']
        ));
        full.append(last);
        assert_eq!(full.size(), 1024);
        assert_eq!(
            full.slice(ByteRange {
                start: 1022,
                end: 1024
            })
            .unwrap(),
            b"ab"
        );
        // Under the limit, parts merge in start order.
        let mut a = SparseBody::new(4);
        assert!(a.insert(ByteRange { start: 2, end: 4 }, b"cd".to_vec()));
        let mut b = SparseBody::new(4);
        assert!(b.insert(ByteRange { start: 0, end: 2 }, b"ab".to_vec()));
        a.append(b);
        assert_eq!(a.slice(ByteRange { start: 0, end: 4 }).unwrap(), b"abcd");
    }
}
