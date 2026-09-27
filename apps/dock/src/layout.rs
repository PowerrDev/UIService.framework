//! Where everything in the Dock goes, from the tile (icon) size alone, in
//! the proportions of the macOS Dock: icons a third of a tile apart, a
//! little room above them, the running-app dots in the band below.

/// What sits in one place along the Dock.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Slot {
    /// An app's tile (the caller's index for it).
    App(usize),
    /// The thin line between the apps, the recent apps and the Trash.
    Divider,
    Trash,
}

/// The Dock's proportions for a tile `tile` pixels square.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Metrics {
    pub tile: u32,
    pub pad_x: u32,
    pub pad_top: u32,
    pub pad_bottom: u32,
    pub gap: u32,
    /// The space a divider takes (the line sits in the middle).
    pub divider: u32,
    pub radius: u32,
    pub dot_radius_x16: u32,
    /// From the tile's bottom edge down to the dot's centre.
    pub dot_offset: u32,
}

impl Metrics {
    pub fn new(tile: u32) -> Self {
        let part = |numerator: u32, denominator: u32| (tile * numerator + denominator / 2) / denominator;
        Self {
            tile,
            pad_x: part(1, 5).max(2),
            pad_top: part(1, 8).max(2),
            pad_bottom: part(1, 5).max(4),
            gap: part(1, 6).max(2),
            divider: part(1, 3).max(3),
            radius: part(3, 8).max(4),
            // A dot about a twelfth of a tile across (in 1/16 pixels).
            dot_radius_x16: (tile * 16 / 24).max(24),
            dot_offset: part(1, 10).max(2),
        }
    }

    pub fn height(&self) -> u32 {
        self.pad_top + self.tile + self.pad_bottom
    }
}

pub const MAX_SLOTS: usize = 24;

/// The Dock's slots laid out left to right.
pub struct Layout {
    pub metrics: Metrics,
    pub width: u32,
    pub height: u32,
    slots: [(Slot, u32); MAX_SLOTS],
    count: usize,
}

impl Layout {
    /// Lay out `slots` (dividers the caller put in, none at either end).
    pub fn new(metrics: Metrics, slots: &[Slot]) -> Self {
        let mut layout = Self {
            metrics,
            width: 0,
            height: metrics.height(),
            slots: [(Slot::Divider, 0); MAX_SLOTS],
            count: 0,
        };
        let mut x = metrics.pad_x;
        let mut previous_was_tile = false;
        for &slot in slots.iter().take(MAX_SLOTS) {
            match slot {
                Slot::Divider => {
                    layout.slots[layout.count] = (slot, x + (metrics.divider.saturating_sub(metrics.gap)) / 2);
                    x += metrics.divider.saturating_sub(metrics.gap);
                    previous_was_tile = false;
                }
                _ => {
                    if previous_was_tile || layout.count > 0 {
                        x += metrics.gap;
                    }
                    layout.slots[layout.count] = (slot, x);
                    x += metrics.tile;
                    previous_was_tile = true;
                }
            }
            layout.count += 1;
        }
        layout.width = x + metrics.pad_x;
        layout
    }

    pub fn slots(&self) -> &[(Slot, u32)] {
        &self.slots[..self.count]
    }

    /// The tile's top edge.
    pub fn tile_top(&self) -> u32 {
        self.metrics.pad_top
    }

    /// The slot index under `x` (Dock coordinates): each tile's column, the
    /// gaps split between neighbours. Dividers are never hit.
    pub fn hit(&self, x: i32, y: i32) -> Option<usize> {
        if y < 0 || y >= self.height as i32 || x < 0 || x >= self.width as i32 {
            return None;
        }
        let half_gap = (self.metrics.gap / 2) as i32;
        self.slots().iter().position(|&(slot, left)| {
            slot != Slot::Divider && x >= left as i32 - half_gap && x < (left + self.metrics.tile) as i32 + half_gap
        })
    }

    /// The horizontal centre of slot `index`.
    pub fn center(&self, index: usize) -> i32 {
        let (slot, left) = self.slots[index];
        match slot {
            Slot::Divider => left as i32,
            _ => (left + self.metrics.tile / 2) as i32,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiles_and_dividers_line_up() {
        let metrics = Metrics::new(96);
        let layout = Layout::new(metrics, &[Slot::App(0), Slot::App(1), Slot::Divider, Slot::Trash]);
        let slots = layout.slots();
        assert_eq!(slots[0], (Slot::App(0), metrics.pad_x));
        assert_eq!(slots[1].1, metrics.pad_x + metrics.tile + metrics.gap);
        // The Trash starts after the divider's space and a gap.
        let divider_space = metrics.divider - metrics.gap;
        assert_eq!(slots[3].1, slots[1].1 + metrics.tile + divider_space + metrics.gap);
        assert_eq!(layout.width, slots[3].1 + metrics.tile + metrics.pad_x);
        assert_eq!(layout.height, metrics.pad_top + 96 + metrics.pad_bottom);
    }

    #[test]
    fn hit_testing_splits_gaps_and_skips_dividers() {
        let metrics = Metrics::new(48);
        let layout = Layout::new(metrics, &[Slot::App(0), Slot::Divider, Slot::App(1)]);
        let first = layout.slots()[0].1 as i32;
        let second = layout.slots()[2].1 as i32;
        assert_eq!(layout.hit(first + 10, 10), Some(0));
        assert_eq!(layout.hit(second + 47, 10), Some(2));
        assert_eq!(layout.hit(-1, 10), None);
        assert_eq!(layout.hit(first + 10, layout.height as i32), None);
        assert_eq!(layout.center(0), first + 24);
    }
}
