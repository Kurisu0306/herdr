//! Space headers for the grouped Agents list.
//!
//! A header is drawn above the first agent of each run of agents from one space and shares that
//! agent's list item. A folded space keeps an item holding only its header.

use super::agent_filter::AgentPanelFilter;
use super::*;

pub(super) struct AgentGroupHeader {
    pub(super) label: String,
    /// Space the header names.
    pub(super) target: AgentPanelFilter,
    /// Present where the space's agents can be folded away.
    pub(super) fold: Option<GroupFold>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct GroupFold {
    pub(super) collapsed: bool,
    /// Agents in the space, shown while they are folded away.
    pub(super) agents: usize,
}

/// One entry of the Agents list: a row, a row under the header that starts its space, or the
/// header alone for a folded space.
pub(super) struct AgentListItem<T> {
    pub(super) header: Option<AgentGroupHeader>,
    pub(super) row: Option<T>,
}

impl<T> AgentListItem<T> {
    pub(super) fn plain(row: T) -> Self {
        Self {
            header: None,
            row: Some(row),
        }
    }
}

/// Turns rows into list items with one header per run that `starts` marks. `header` sees the
/// first row of a run and the run's length; when it reports the space as folded, the run's rows
/// are dropped and the header stands alone.
pub(super) fn grouped_items<T>(
    rows: Vec<T>,
    starts: &[bool],
    mut header: impl FnMut(&T, usize) -> AgentGroupHeader,
) -> Vec<AgentListItem<T>> {
    if starts.is_empty() {
        return rows.into_iter().map(AgentListItem::plain).collect();
    }
    let mut items = Vec::with_capacity(rows.len());
    let mut rows = rows.into_iter().enumerate().peekable();
    while let Some((_, first)) = rows.next() {
        let mut run = vec![first];
        while let Some((_, row)) = rows.next_if(|(index, _)| !starts[*index]) {
            run.push(row);
        }
        let head = header(&run[0], run.len());
        let folded = head.fold.is_some_and(|fold| fold.collapsed);
        let mut run = run.into_iter();
        items.push(AgentListItem {
            header: Some(head),
            row: run.next().filter(|_| !folded),
        });
        if !folded {
            items.extend(run.map(AgentListItem::plain));
        }
    }
    items
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

/// Heights and trailing gaps of the list items, each given as whether it has a header and how
/// many lines its row takes. A header adds one line and is kept at least one blank line away
/// from the rows of the space before it.
pub(super) fn list_layout(
    items: impl Iterator<Item = (bool, Option<usize>)>,
    row_gap: u16,
) -> (Vec<u16>, Vec<u16>) {
    let items = items.collect::<Vec<_>>();
    let heights = items
        .iter()
        .map(|(header, lines)| {
            let lines = lines.map_or(0, |lines| lines.clamp(1, usize::from(u16::MAX) - 1) as u16);
            lines + u16::from(*header)
        })
        .collect::<Vec<_>>();
    let gaps = (0..items.len())
        .map(|index| match (items[index], items.get(index + 1)) {
            (_, None) => 0,
            ((_, Some(_)), Some((true, _))) => row_gap.max(1),
            // Folded spaces stack without the blank line a list of rows needs.
            ((_, None), Some((true, _))) => 0,
            (_, Some((false, _))) => row_gap,
        })
        .collect();
    (heights, gaps)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(collapsed: bool) -> AgentGroupHeader {
        AgentGroupHeader {
            label: "space".into(),
            target: AgentPanelFilter {
                endpoint_id: ClientEndpointId::Local,
                space: "space".into(),
            },
            fold: Some(GroupFold {
                collapsed,
                agents: 0,
            }),
        }
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
    fn header_adds_a_line_and_separates_spaces() {
        let items = [(true, Some(2)), (false, Some(2)), (true, Some(1))];

        let (heights, gaps) = list_layout(items.into_iter(), 0);

        assert_eq!(heights, [3, 2, 2]);
        assert_eq!(gaps, [0, 1, 0]);
    }

    #[test]
    fn layout_without_headers_matches_the_plain_list() {
        let items = [(false, Some(2)), (false, Some(0)), (false, Some(1))];

        let (heights, gaps) = list_layout(items.into_iter(), 1);

        assert_eq!(heights, [2, 1, 1]);
        assert_eq!(gaps, [1, 1, 0]);
    }

    #[test]
    fn folded_spaces_take_one_line_each_and_stack() {
        let items = [(true, None), (true, None), (true, Some(1))];

        let (heights, gaps) = list_layout(items.into_iter(), 0);

        assert_eq!(heights, [1, 1, 2]);
        assert_eq!(gaps, [0, 0, 0]);
    }

    #[test]
    fn folded_run_keeps_its_header_and_drops_its_rows() {
        let rows = vec!["a1", "a2", "b1"];
        let starts = group_starts(rows.iter().map(|row| &row[..1]));

        let items = grouped_items(rows, &starts, |row, _| header(row.starts_with('a')));

        let shape = items
            .iter()
            .map(|item| (item.header.is_some(), item.row))
            .collect::<Vec<_>>();
        assert_eq!(shape, [(true, None), (true, Some("b1"))]);
    }

    #[test]
    fn rows_without_groups_become_plain_items() {
        let items = grouped_items(vec!["a1", "a2"], &[], |_, _| header(false));

        assert!(items
            .iter()
            .all(|item| item.header.is_none() && item.row.is_some()));
    }
}
