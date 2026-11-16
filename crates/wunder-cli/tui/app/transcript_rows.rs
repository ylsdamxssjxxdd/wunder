//! How many rows each card takes, measured once and reused.
//!
//! The transcript is one document the app owns end to end: the window is chosen by
//! walking card row counts from the tail, so a frame costs a *lookup* per card instead
//! of a measurement. Wrapping is the expensive part (markdown parsing, word wrap, a tool
//! card's own layout), and a settled card cannot change how many rows it takes until its
//! text, its fold state or the terminal width does.
//!
//! So this ledger keeps one count per card, parallel to the log. The width, the raw flag,
//! the fold state and a stamp of the card's own text are compared per read; a card that
//! is still streaming or waiting on a tool result is never cached at all.

/// One card's row budget under the width and fold state it was measured with.
#[derive(Clone, Copy, Default)]
struct RowCount {
    width: u16,
    raw: bool,
    expanded: bool,
    stamp: u64,
    rows: usize,
}

#[derive(Default)]
pub(super) struct TranscriptRows {
    rows: Vec<RowCount>,
}

impl TranscriptRows {
    /// Keep the ledger one row per card. A count that disagrees with the log means the
    /// log was rebuilt underneath (a clear, a trim at the far end this ledger never saw),
    /// and the only safe answer is to forget and measure again.
    pub(super) fn sync(&mut self, cards: usize) {
        match self.rows.len().cmp(&cards) {
            std::cmp::Ordering::Equal => {}
            std::cmp::Ordering::Less => self.rows.resize(cards, RowCount::default()),
            std::cmp::Ordering::Greater => self.rows.truncate(cards),
        }
    }

    pub(super) fn get(
        &self,
        index: usize,
        width: u16,
        raw: bool,
        expanded: bool,
        stamp: u64,
    ) -> Option<usize> {
        let count = self.rows.get(index)?;
        (count.width == width
            && count.raw == raw
            && count.expanded == expanded
            && count.stamp == stamp)
            .then_some(count.rows)
    }

    pub(super) fn put(
        &mut self,
        index: usize,
        width: u16,
        raw: bool,
        expanded: bool,
        stamp: u64,
        rows: usize,
    ) {
        if let Some(slot) = self.rows.get_mut(index) {
            *slot = RowCount {
                width,
                raw,
                expanded,
                stamp,
                rows,
            };
        }
    }

    /// A card left the transcript, so every count behind it belongs to the card in front.
    pub(super) fn remove(&mut self, index: usize) {
        if index < self.rows.len() {
            self.rows.remove(index);
        }
    }

    pub(super) fn clear(&mut self) {
        self.rows.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_settled_count_survives_a_second_ask_and_answers_to_its_key() {
        let mut ledger = TranscriptRows::default();
        ledger.sync(2);
        assert_eq!(ledger.get(0, 80, false, false, 7), None);
        ledger.put(0, 80, false, false, 7, 4);
        assert_eq!(ledger.get(0, 80, false, false, 7), Some(4));
        assert_eq!(
            ledger.get(0, 40, false, false, 7),
            None,
            "width is part of the key"
        );
        assert_eq!(
            ledger.get(0, 80, true, false, 7),
            None,
            "raw mode is part of the key"
        );
        assert_eq!(
            ledger.get(0, 80, false, true, 7),
            None,
            "fold state is part of the key"
        );
        assert_eq!(
            ledger.get(0, 80, false, false, 8),
            None,
            "a changed card is remeasured"
        );
    }

    #[test]
    fn removing_a_card_moves_every_count_behind_it() {
        let mut ledger = TranscriptRows::default();
        ledger.sync(3);
        for index in 0..3 {
            ledger.put(index, 80, false, false, 1, index + 1);
        }
        ledger.remove(0);
        assert_eq!(ledger.get(0, 80, false, false, 1), Some(2));
        assert_eq!(ledger.get(1, 80, false, false, 1), Some(3));
        assert_eq!(ledger.get(2, 80, false, false, 1), None);
    }
}
