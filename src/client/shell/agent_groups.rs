//! Space headers for the grouped Agents list.
//!
//! A header is drawn above the first agent of each run of agents from one space. It belongs to
//! that agent's list item instead of being an item of its own, so item indices stay agent
//! indices for scrolling, reveal, and indexed focus.

use super::agent_filter::AgentPanelFilter;
use super::*;

pub(super) struct AgentGroupHeader {
    pub(super) label: String,
    /// Space the header names; clicking it narrows the panel to that space.
    pub(super) target: AgentPanelFilter,
}

/// Whether the panel order keeps each space's agents together. Priority order and server-owned
/// agent views interleave spaces, where a header per run would only add noise.
pub(super) fn grouped_order(config: &ClientShellConfig, agent_view_label: Option<&str>) -> bool {
    config.agent_panel_sort == crate::config::AgentPanelSortConfig::Spaces
        && agent_view_label.is_none()
}

/// Marks the rows that start a new space. Empty when the rows span fewer than two spaces, where
/// a lone header would repeat what every row already says.
pub(super) fn group_starts<K: PartialEq>(keys: impl Iterator<Item = K>) -> Vec<bool> {
    let keys = keys.collect::<Vec<_>>();
    let starts = (0..keys.len())
        .map(|index| index == 0 || keys[index] != keys[index - 1])
        .collect::<Vec<_>>();
    if starts.iter().filter(|start| **start).count() < 2 {
        return Vec::new();
    }
    starts
}

/// For each key, the position at which that key first appeared.
pub(super) fn first_appearance_ranks<K: PartialEq>(keys: impl Iterator<Item = K>) -> Vec<usize> {
    let mut seen = Vec::new();
    keys.map(|key| {
        seen.iter()
            .position(|known| *known == key)
            .unwrap_or_else(|| {
                seen.push(key);
                seen.len() - 1
            })
    })
    .collect()
}

/// Moves each item next to the first item of its rank, keeping every other relative order.
/// Same-named spaces need not be neighbours in the Spaces panel, but they are one group here.
pub(super) fn gather_by_rank<T>(items: &mut Vec<T>, ranks: Vec<usize>) {
    if ranks.is_sorted() {
        return;
    }
    let mut ranked = std::mem::take(items)
        .into_iter()
        .zip(ranks)
        .collect::<Vec<_>>();
    ranked.sort_by_key(|(_, rank)| *rank);
    *items = ranked.into_iter().map(|(item, _)| item).collect();
}

/// Heights and trailing gaps of the list items. A header adds one row to the item it starts and
/// keeps at least one blank row between it and the previous space.
pub(super) fn list_layout(
    row_lines: impl Iterator<Item = usize>,
    headers: &[Option<AgentGroupHeader>],
    row_gap: u16,
) -> (Vec<u16>, Vec<u16>) {
    let starts = |index: usize| headers.get(index).is_some_and(Option::is_some);
    let heights = row_lines
        .enumerate()
        .map(|(index, lines)| {
            let lines = lines.clamp(1, usize::from(u16::MAX) - 1) as u16;
            lines + u16::from(starts(index))
        })
        .collect::<Vec<_>>();
    let gaps = (0..heights.len())
        .map(|index| {
            if index + 1 == heights.len() {
                0
            } else if starts(index + 1) {
                row_gap.max(1)
            } else {
                row_gap
            }
        })
        .collect();
    (heights, gaps)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header() -> Option<AgentGroupHeader> {
        Some(AgentGroupHeader {
            label: "space".into(),
            target: AgentPanelFilter {
                endpoint_id: ClientEndpointId::Local,
                space: "space".into(),
            },
        })
    }

    #[test]
    fn starts_mark_each_run_of_one_space() {
        assert_eq!(
            group_starts(["a", "a", "b", "a"].into_iter()),
            [true, false, true, true]
        );
    }

    #[test]
    fn one_space_gets_no_header() {
        assert!(group_starts(["a", "a"].into_iter()).is_empty());
        assert!(group_starts(std::iter::empty::<&str>()).is_empty());
    }

    #[test]
    fn gathering_joins_separated_runs_without_reordering_the_rest() {
        let mut items = vec!["home-1", "work-1", "home-2", "play-1", "work-2"];
        let ranks = first_appearance_ranks(items.iter().map(|item| &item[..4]));
        assert_eq!(ranks, [0, 1, 0, 2, 1]);

        gather_by_rank(&mut items, ranks);

        assert_eq!(items, ["home-1", "home-2", "work-1", "work-2", "play-1"]);
    }

    #[test]
    fn header_adds_a_row_and_separates_spaces() {
        let headers = [header(), None, header()];

        let (heights, gaps) = list_layout([2, 2, 1].into_iter(), &headers, 0);

        assert_eq!(heights, [3, 2, 2]);
        assert_eq!(gaps, [0, 1, 0]);
    }

    #[test]
    fn layout_without_headers_matches_the_plain_list() {
        let (heights, gaps) = list_layout([2, 0, 1].into_iter(), &[], 1);

        assert_eq!(heights, [2, 1, 1]);
        assert_eq!(gaps, [1, 1, 0]);
    }
}
