//! Judging a table cell's value after a committed write.

/// What a cell shows after a committed write, next to the value written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReadBack {
    Matches,
    Empty,
    Differs,
}

/// Judges the cell's value after a commit. A cell that publishes nothing while
/// a value was written took no text; a cell that shows something else may hold
/// a formatted form of it.
pub(crate) fn classify(current: Option<&str>, wanted: &str) -> ReadBack {
    match current {
        Some(held) if held == wanted => ReadBack::Matches,
        Some(held) if !held.is_empty() => ReadBack::Differs,
        _ if wanted.is_empty() => ReadBack::Matches,
        _ => ReadBack::Empty,
    }
}

#[cfg(test)]
#[path = "cell_read_back_tests.rs"]
mod tests;
