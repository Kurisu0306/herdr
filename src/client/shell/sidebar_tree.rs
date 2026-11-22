//! Tree sidebar: every space once, followed by its agents.
//!
//! The split sidebar lists spaces in one panel and agents in another, which names each space
//! two or three times. Here a space row is the header of its own agents, so the name appears
//! once and the rest of the row width goes to the agents.

use super::agent_filter::{self, AgentPanelFilter};
use super::agent_sidebar::{self, AgentListOrder, AgentRow};
use super::render::{put_right_text, put_text, render_sidebar_background, ShellRenderState};
use super::*;

/// Rows above the chip strip: the panel title.
const TITLE_ROWS: u16 = 1;
/// Cells an agent row is indented under its space.
const AGENT_INDENT: u16 = 2;

/// Workspaces listed under one name. Same-named workspaces are one project to the user, so they
/// share a row; the row acts on the focused member, or the first one.
struct SpaceGroup<'a> {
    name: &'a str,
    workspace: &'a ClientShellWorkspace,
    workspace_index: usize,
    status: crate::api::schema::AgentStatus,
    focused: bool,
    selected: bool,
    agent_count: usize,
    collapsed: bool,
}

enum TreeEntry<'a> {
    Space(SpaceGroup<'a>, Vec<Vec<crate::ui::ResolvedToken>>),
    Agent(AgentRow),
}

impl TreeEntry<'_> {
    fn lines(&self) -> usize {
        match self {
            Self::Space(_, rows) => rows.len(),
            Self::Agent(row) => row.rows.len(),
        }
    }
}

fn tree_entries<'a>(
    snapshot: &'a ClientShellSnapshot,
    config: &ClientShellConfig,
    state: &ShellRenderState<'_>,
    spaces: &[(&'a ClientShellWorkspace, &'a str)],
    filter: Option<&AgentPanelFilter>,
) -> Vec<TreeEntry<'a>> {
    // Every agent the list could show, so a collapsed space can still say how many it hides.
    let nothing_collapsed = HashSet::new();
    let now_ms = crate::agent_state_time::unix_ms_now();
    let mut agents = agent_sidebar::ordered_agent_pane_ids_among(
        snapshot,
        spaces,
        AgentListOrder {
            sort: config.agent_panel_sort,
            space: filter.map(|filter| filter.space.as_str()),
            tree: Some(&nothing_collapsed),
        },
    )
    .into_iter()
    .filter_map(|pane_id| agent_sidebar::agent_row(snapshot, &pane_id, config, None, now_ms))
    .map(Some)
    .collect::<Vec<_>>();

    let mut names = Vec::<&str>::new();
    for (_, name) in spaces {
        if !names.contains(name) && filter.is_none_or(|filter| filter.space == *name) {
            names.push(name);
        }
    }
    let mut entries = Vec::new();
    for name in names {
        let members = spaces
            .iter()
            .filter(|(_, member)| *member == name)
            .map(|(workspace, _)| *workspace)
            .collect::<Vec<_>>();
        let Some(workspace) = members
            .iter()
            .find(|workspace| workspace.focused)
            .or(members.first())
            .copied()
        else {
            continue;
        };
        let Some(workspace_index) = snapshot
            .workspaces
            .iter()
            .position(|candidate| candidate.workspace_id == workspace.workspace_id)
        else {
            continue;
        };
        let status = members
            .iter()
            .map(|workspace| workspace.agent_status)
            .max_by_key(|status| status_priority(*status))
            .unwrap_or(workspace.agent_status);
        let mut rows = Vec::new();
        for slot in &mut agents {
            if slot
                .as_ref()
                .is_some_and(|row| agent_filter::space_name(spaces, &row.workspace_id) == name)
            {
                rows.extend(slot.take());
            }
        }
        let group = SpaceGroup {
            name,
            workspace,
            workspace_index,
            status,
            focused: members.iter().any(|workspace| workspace.focused),
            selected: state.selected_workspace_id.is_some_and(|target| {
                members.iter().any(|workspace| {
                    target.matches(state.active_endpoint_id, &workspace.workspace_id)
                })
            }),
            agent_count: rows.len(),
            collapsed: config.collapsed_spaces.contains(name),
        };
        let collapsed = group.collapsed;
        let space_rows =
            super::sidebar::workspace_rows_named(workspace, name, status, false, &config.spaces);
        entries.push(TreeEntry::Space(group, space_rows));
        if !collapsed {
            entries.extend(rows.into_iter().map(|mut row| {
                // The space row above already names the space.
                row.drop_space_only_rows();
                TreeEntry::Agent(row)
            }));
        }
    }
    entries
}

/// Blank rows after each entry: agents are set off from the next space, and each kind keeps its
/// own configured gap among its peers.
fn entry_gaps(entries: &[TreeEntry<'_>], config: &ClientShellConfig) -> Vec<u16> {
    entries
        .iter()
        .enumerate()
        .map(|(index, entry)| match (entry, entries.get(index + 1)) {
            (_, None) | (TreeEntry::Space(..), Some(TreeEntry::Agent(_))) => 0,
            (TreeEntry::Agent(_), Some(TreeEntry::Agent(_))) => config.agents.row_gap,
            (TreeEntry::Agent(_), Some(TreeEntry::Space(..))) => config.agents.row_gap.max(1),
            (TreeEntry::Space(..), Some(TreeEntry::Space(..))) => config.spaces.row_gap,
        })
        .collect()
}

pub(super) fn render(
    buffer: &mut Buffer,
    area: Rect,
    snapshot: &ClientShellSnapshot,
    config: &ClientShellConfig,
    state: &mut ShellRenderState<'_>,
    hits: &mut ShellHitMap,
) {
    let palette = &config.palette;
    render_sidebar_background(buffer, area, palette);
    hits.sidebar_divider = if area.is_empty() {
        Rect::default()
    } else {
        Rect::new(area.right().saturating_sub(1), area.y, 1, area.height)
    };
    let content = Rect::new(area.x, area.y, area.width.saturating_sub(1), area.height);
    if content.is_empty() {
        return;
    }
    put_text(
        buffer,
        content.x,
        content.y,
        content.width,
        " spaces",
        Style::default()
            .fg(palette.overlay0)
            .add_modifier(Modifier::BOLD),
    );

    let endpoint_id = state.active_endpoint_id;
    let spaces = agent_filter::space_labels(snapshot);
    let filter = config
        .agent_panel_filter
        .as_ref()
        .filter(|filter| filter.space_among(endpoint_id, &spaces).is_some());
    // The last row belongs to the footer controls.
    let list_area = Rect::new(
        content.x,
        content.y,
        content.width,
        content.height.saturating_sub(1),
    );
    let list_offset = agent_filter::render_chips(
        buffer,
        list_area,
        TITLE_ROWS,
        || agent_filter::snapshot_chips(endpoint_id, snapshot, &spaces, filter),
        filter,
        config,
        hits,
    );
    let body = Rect::new(
        list_area.x,
        list_area.y.saturating_add(list_offset),
        list_area.width,
        list_area.height.saturating_sub(list_offset),
    );
    // The tree is one list, scrolled and revealed through the Agents list's state.
    hits.agent_body = body;

    let entries = tree_entries(snapshot, config, state, &spaces, filter);
    let heights = entries
        .iter()
        .map(|entry| entry.lines().clamp(1, usize::from(u16::MAX)) as u16)
        .collect::<Vec<_>>();
    let gaps = entry_gaps(&entries, config);
    if !body.is_empty() {
        let reveal_space = std::mem::take(state.reveal_focused_workspace)
            | std::mem::take(state.reveal_navigation_workspace);
        let reveal_agent = state.reveal_tree_agent.take();
        let navigating = entries
            .iter()
            .any(|entry| matches!(entry, TreeEntry::Space(group, _) if group.selected));
        let target = reveal_agent
            .and_then(|pane_id| {
                entries.iter().position(
                    |entry| matches!(entry, TreeEntry::Agent(row) if row.pane_id == pane_id),
                )
            })
            .or_else(|| {
                reveal_space
                    .then(|| {
                        entries.iter().position(|entry| {
                            matches!(entry, TreeEntry::Space(group, _)
                                if if navigating { group.selected } else { group.focused })
                        })
                    })
                    .flatten()
            });
        if let Some(target) = target {
            *state.agent_scroll = super::scroll::list_scroll_start_to_reveal(
                &heights,
                &gaps,
                body.height,
                *state.agent_scroll,
                target,
            );
        }
    }
    let metrics =
        super::scroll::list_scroll_metrics(&heights, &gaps, body.height, *state.agent_scroll);
    hits.agent_max_scroll = metrics.max_offset_from_bottom;
    hits.agent_scroll_metrics = Some(metrics);
    *state.agent_scroll = metrics
        .max_offset_from_bottom
        .saturating_sub(metrics.offset_from_bottom);
    let show_scrollbar = metrics.max_offset_from_bottom > 0 && body.width > 1;
    let content_width = body.width.saturating_sub(u16::from(show_scrollbar));
    let navigating = state.selected_workspace_id.is_some();

    let mut y = body.y;
    for (index, entry) in entries.iter().enumerate().skip(*state.agent_scroll) {
        let height = heights[index].min(body.height);
        if body.is_empty() || y.saturating_add(height) > body.bottom() {
            break;
        }
        let rect = Rect::new(body.x, y, content_width, height);
        match entry {
            TreeEntry::Space(group, rows) => {
                render_space_row(buffer, rect, group, rows.clone(), navigating, config, hits)
            }
            TreeEntry::Agent(row) => {
                if row.focused {
                    buffer.set_style(rect, Style::default().bg(palette.active_row_bg));
                }
                hits.agents.push((rect, row.pane_id.clone()));
                let indent = AGENT_INDENT.min(rect.width);
                hits.agent_ages_shown |= agent_sidebar::render_agent_row(
                    buffer,
                    Rect::new(rect.x + indent, rect.y, rect.width - indent, rect.height),
                    row,
                    config,
                );
            }
        }
        y = y.saturating_add(height).saturating_add(gaps[index]);
    }
    if show_scrollbar {
        let track = Rect::new(body.right().saturating_sub(1), body.y, 1, body.height);
        hits.agent_scrollbar = track;
        super::scroll::render_list_scrollbar(buffer, track, metrics, palette);
    }

    // The sidebar toggle takes the last two cells of the footer row.
    super::sidebar::render_sidebar_footer(
        buffer,
        Rect::new(
            content.x,
            content.y,
            content.width.saturating_sub(2),
            content.height,
        ),
        snapshot,
        config,
        hits,
    );
    hits.sidebar_toggle = Rect::new(
        area.right().saturating_sub(2),
        area.bottom().saturating_sub(1),
        u16::from(area.width > 1),
        u16::from(area.height > 0),
    );
    put_text(
        buffer,
        hits.sidebar_toggle.x,
        hits.sidebar_toggle.y,
        hits.sidebar_toggle.width,
        "«",
        Style::default().fg(palette.overlay0),
    );
}

fn render_space_row(
    buffer: &mut Buffer,
    rect: Rect,
    group: &SpaceGroup<'_>,
    rows: Vec<Vec<crate::ui::ResolvedToken>>,
    navigating: bool,
    config: &ClientShellConfig,
    hits: &mut ShellHitMap,
) {
    let palette = &config.palette;
    super::sidebar::render_workspace_rows(
        buffer,
        rect,
        group.status,
        config.status_indicators,
        &WorkspaceEntry {
            index: group.workspace_index,
            indented: false,
            last_child: false,
        },
        rows,
        group.focused,
        group.selected,
        navigating,
        false,
        palette,
    );
    hits.workspaces.push(WorkspaceHit {
        rect,
        endpoint_id: ClientEndpointId::Local,
        workspace_id: group.workspace.workspace_id.clone(),
        indented: false,
        group_toggle: None,
    });
    if group.agent_count == 0 || rect.is_empty() {
        return;
    }
    // A collapsed space says how many agents it hides, next to the control that shows them.
    let control = if group.collapsed {
        format!("{} ▸", group.agent_count)
    } else {
        "▾".to_owned()
    };
    let first_row = Rect::new(rect.x, rect.y, rect.width, 1);
    put_right_text(
        buffer,
        first_row,
        rect.y,
        &control,
        Style::default().fg(palette.accent),
    );
    if config.mouse_capture {
        let width = super::render::display_width(&control)
            .saturating_add(1)
            .min(rect.width);
        hits.space_collapse_toggles.push((
            Rect::new(rect.right() - width, rect.y, width, 1),
            group.name.to_owned(),
        ));
    }
}
