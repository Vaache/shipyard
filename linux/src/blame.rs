use std::rc::Rc;

use crate::api::models::BlameRange;

pub const NO_RANGE: u32 = u32::MAX;

pub fn line_map(ranges: &[(i64, i64)], line_count: usize) -> Vec<u32> {
    let mut map = vec![NO_RANGE; line_count];
    for (i, &(start, end)) in ranges.iter().enumerate() {
        if start < 1 || end < start {
            continue;
        }
        let from = (start - 1) as usize;
        let to = (end as usize).min(line_count);
        for slot in map.iter_mut().take(to).skip(from) {
            *slot = i as u32;
        }
    }
    map
}

pub fn heat(age: i64) -> f64 {
    let age = age.clamp(1, 10) as f64;
    0.15 + 0.85 * (10.0 - age) / 9.0
}

pub fn fit(text: &str, width: usize) -> String {
    let count = text.chars().count();
    if count <= width {
        return format!("{text}{}", " ".repeat(width - count));
    }
    let mut out: String = text.chars().take(width.saturating_sub(1)).collect();
    out.push('…');
    out
}

pub struct Blame {
    pub ranges: Rc<Vec<BlameRange>>,
    map: Vec<u32>,
}

impl Blame {
    pub fn new(ranges: Rc<Vec<BlameRange>>, line_count: usize) -> Blame {
        let spans: Vec<(i64, i64)> = ranges.iter().map(|r| (r.starting_line, r.ending_line)).collect();
        let map = line_map(&spans, line_count);
        Blame { ranges, map }
    }

    pub fn index_at(&self, line: u32) -> Option<usize> {
        self.map.get(line as usize).copied().filter(|&i| i != NO_RANGE).map(|i| i as usize)
    }

    pub fn range_at(&self, line: u32) -> Option<&BlameRange> {
        self.index_at(line).and_then(|i| self.ranges.get(i))
    }

    pub fn starts_range(&self, line: u32) -> bool {
        let Some(r) = self.range_at(line) else { return false };
        line == 0 || self.range_at(line - 1).is_none_or(|p| p.commit.oid != r.commit.oid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::models::{BlameAuthor, BlameCommit};

    fn range(start: i64, end: i64, oid: &str) -> BlameRange {
        BlameRange {
            starting_line: start,
            ending_line: end,
            age: 5,
            commit: BlameCommit {
                oid: oid.into(),
                abbreviated_oid: oid.into(),
                message_headline: String::new(),
                committed_date: None,
                url: String::new(),
                author: Some(BlameAuthor { name: Some("a".into()), user: None }),
            },
        }
    }

    #[test]
    fn maps_ranges_to_lines() {
        assert_eq!(line_map(&[(1, 2), (3, 5)], 5), vec![0, 0, 1, 1, 1]);
        assert_eq!(line_map(&[(2, 3)], 4), vec![NO_RANGE, 0, 0, NO_RANGE]);
        assert_eq!(line_map(&[(1, 10)], 3), vec![0, 0, 0]);
        assert_eq!(line_map(&[(0, 2), (3, 2), (2, 2)], 3), vec![NO_RANGE, 2, NO_RANGE]);
        assert!(line_map(&[(1, 3)], 0).is_empty());
    }

    #[test]
    fn range_lookup_and_starts() {
        let b = Blame::new(Rc::new(vec![range(1, 2, "a"), range(3, 3, "b"), range(4, 6, "a")]), 6);
        assert_eq!(b.range_at(0).map(|r| r.commit.oid.as_str()), Some("a"));
        assert_eq!(b.range_at(2).map(|r| r.commit.oid.as_str()), Some("b"));
        assert_eq!(b.index_at(5), Some(2));
        assert_eq!(b.index_at(6), None);
        let starts: Vec<u32> = (0..6).filter(|&l| b.starts_range(l)).collect();
        assert_eq!(starts, vec![0, 2, 3]);
        let same = Blame::new(Rc::new(vec![range(1, 2, "a"), range(3, 4, "a"), range(6, 6, "b")]), 7);
        let starts: Vec<u32> = (0..7).filter(|&l| same.starts_range(l)).collect();
        assert_eq!(starts, vec![0, 5]);
    }

    #[test]
    fn heat_and_fit() {
        assert!(heat(1) > heat(5));
        assert!(heat(5) > heat(10));
        assert!((heat(1) - 1.0).abs() < 1e-9);
        assert!((heat(42) - heat(10)).abs() < 1e-9);
        assert_eq!(fit("bob", 5), "bob  ");
        assert_eq!(fit("alexandra", 5), "alex…");
        assert_eq!(fit("abcde", 5), "abcde");
    }
}
