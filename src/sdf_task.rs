use std::cmp::Eq;
use std::cmp::Ord;
use std::cmp::Ordering;

use crate::sdf_algorithm::DstT;

#[derive(Copy, Clone)]
pub struct SdfTask {
    pub x: u32,
    pub y: u32,
    pub level: u8,
    pub best_case_dst_sqr: DstT,
}

impl PartialEq for SdfTask {
    fn eq(&self, other: &Self) -> bool {
        self.x == other.x
            && self.y == other.y
            && self.level == other.level
            && self.best_case_dst_sqr == other.best_case_dst_sqr
    }
}

impl Eq for SdfTask {}

impl PartialOrd for SdfTask {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SdfTask {
    fn cmp(&self, other: &Self) -> Ordering {
        // order swapped because we want a min-Heap
        other
            .best_case_dst_sqr
            .partial_cmp(&self.best_case_dst_sqr)
            .expect("Infinite or NaN distance shouldn't be possible for our use case")
    }
}
