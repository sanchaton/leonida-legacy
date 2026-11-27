//! Dependencies on texture readbacks still to land in title memory.
//!
//! Unlike a shader's broad writable view, a readback describes bytes that
//! will be copied. Submission preparation must not speculate past them.
//! This orders separate submissions; it does not split dependencies between
//! commands that are already grouped into one submission.

/// The title memory a submission-time step reads or overwrites.
#[derive(Clone, Copy)]
pub(crate) enum Access {
    Range(usize, usize),
    Copy { dst: usize, src: usize, len: usize },
    Unknown,
    HostOnly,
}

/// Exact destination ranges, kept until their flights have landed.
#[derive(Clone, Default)]
pub(crate) struct ReadBacks(Vec<(usize, usize)>);

impl ReadBacks {
    pub(crate) const fn new() -> Self {
        Self(Vec::new())
    }

    pub(crate) fn ranges(&self) -> &[(usize, usize)] {
        &self.0
    }

    pub(crate) fn extend(&mut self, ranges: impl IntoIterator<Item = (usize, usize)>) {
        join_ranges(&mut self.0, ranges);
    }

    /// Only written bytes, not the padding between the title's rows.
    pub(crate) fn rows(&mut self, at: usize, [row, rows, pitch]: [usize; 3]) -> bool {
        if row == 0 || rows == 0 {
            return true;
        }
        if (rows - 1)
            .checked_mul(pitch)
            .and_then(|offset| at.checked_add(offset))
            .and_then(|last| last.checked_add(row))
            .is_none()
        {
            return false;
        }
        self.extend((0..rows).map(|line| {
            let from = at + line * pitch;
            (from, from + row)
        }));
        true
    }

    pub(crate) fn clear(&mut self) {
        self.0.clear();
    }

    /// This check precedes the optional read-ahead of shader-write ranges.
    pub(crate) fn blocks(&self, access: Access) -> bool {
        let hits = |at: usize, len: usize| touches(&self.0, at, at.saturating_add(len));
        match access {
            Access::Range(at, len) => hits(at, len),
            Access::Copy { dst, src, len } => hits(src, len) || hits(dst, len),
            Access::Unknown => !self.0.is_empty(),
            Access::HostOnly => false,
        }
    }
}

/// Whether `[from, to)` touches a sorted, joined range.
pub(crate) fn touches(ranges: &[(usize, usize)], from: usize, to: usize) -> bool {
    if from >= to {
        return false;
    }
    let at = ranges.partition_point(|r| r.1 <= from);
    ranges.get(at).is_some_and(|r| r.0 < to)
}

/// Adds ranges in order, joining overlapping or adjacent spans.
pub(crate) fn join_ranges(
    ranges: &mut Vec<(usize, usize)>,
    more: impl IntoIterator<Item = (usize, usize)>,
) {
    ranges.extend(more.into_iter().filter(|r| r.0 < r.1));
    ranges.sort_unstable();
    let mut out: Vec<(usize, usize)> = Vec::with_capacity(ranges.len());
    for &(from, to) in ranges.iter() {
        match out.last_mut() {
            Some(last) if from <= last.1 => last.1 = last.1.max(to),
            _ => out.push((from, to)),
        }
    }
    *ranges = out;
}

#[cfg(test)]
mod tests {
    use super::{Access, ReadBacks};

    /// Two separate submissions: the first texture copy has not landed
    /// when the second's CPU-side buffer copy is prepared. Its source must
    /// become visible first, even with speculative shader fills enabled.
    #[test]
    fn later_submission_copies_the_readback_result() {
        let mut pending = ReadBacks::default();
        pending.rows(0x1000, [4, 1, 256]);
        let access = Access::Copy {
            dst: 0x2000,
            src: 0x1000,
            len: 4,
        };
        let mut source = 0;
        let mut copied = None;
        // This is the mandatory production decision, before fill_ahead.
        if !pending.blocks(access) {
            copied = Some(source);
        }
        assert_eq!(copied, None, "copy prepared before its producer landed");
        source = 37;
        pending.clear();
        if !pending.blocks(access) {
            copied = Some(source);
        }
        assert_eq!(copied, Some(37));
    }

    #[test]
    fn overwritten_readback_and_cpu_uploads_wait_too() {
        let mut pending = ReadBacks::default();
        pending.rows(0x1000, [16, 1, 256]);
        assert!(pending.blocks(Access::Copy {
            dst: 0x1008,
            src: 0x2000,
            len: 4
        }));
        // Fills, shadow refreshes, write snapshots and texture staging all
        // read a range of title memory during submission preparation.
        assert!(pending.blocks(Access::Range(0x1008, 4)));
        assert!(pending.blocks(Access::Unknown));
        assert!(!pending.blocks(Access::HostOnly));
        assert!(!pending.blocks(Access::Range(0x2000, 16)));
    }

    #[test]
    fn pitched_rows_do_not_block_padding_or_unrelated_memory() {
        let mut pending = ReadBacks::default();
        pending.rows(0x1000, [8, 2, 256]);
        assert!(pending.blocks(Access::Range(0x1004, 4)));
        assert!(pending.blocks(Access::Range(0x1104, 4)));
        assert!(!pending.blocks(Access::Range(0x1008, 248)));
        assert!(!pending.blocks(Access::Range(0x0ff0, 16)));
        assert!(!pending.blocks(Access::Range(0x1108, 16)));
        assert!(!pending.blocks(Access::Range(0x1000, 0)));
    }

    #[test]
    fn empty_and_overflowing_footprints_do_not_add_partial_ranges() {
        let mut pending = ReadBacks::new();
        assert!(pending.rows(usize::MAX, [0, 2, 256]));
        assert!(pending.rows(usize::MAX, [16, 0, 256]));
        assert!(!pending.rows(usize::MAX - 3, [8, 1, 256]));
        assert!(!pending.rows(0x1000, [8, usize::MAX, 256]));
        assert!(!pending.blocks(Access::Unknown));
    }

    #[test]
    fn landing_one_flight_keeps_later_readbacks_pending() {
        let mut first = ReadBacks::default();
        first.rows(0x1000, [16, 1, 256]);
        let mut later = ReadBacks::default();
        later.rows(0x1008, [16, 1, 256]);
        later.extend([(0x3000, 0x3100)]); // A tiled texture readback.
        let mut pending = first.clone();
        pending.extend(later.ranges().iter().copied());
        // Removing flights from the queue to land them does not clear the
        // pending ranges: a CPU readback can still be in progress.
        assert!(pending.blocks(Access::Range(0x1000, 4)));
        // After the first flight's copy-back, rebuild from those still in
        // flight. Overlap with the landed flight must not erase the later one.
        pending.clear();
        pending.extend(later.ranges().iter().copied());
        assert!(!pending.blocks(Access::Range(0x1000, 4)));
        assert!(pending.blocks(Access::Range(0x1008, 4)));
        assert!(pending.blocks(Access::Range(0x3000, 4)));
    }
}
