use std::collections::HashMap;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::Line,
    widgets::{Paragraph, Widget},
};

use super::*;

pub(super) struct AgentRow {
    pub(super) pane_id: String,
    pub(super) workspace_id: String,
    /// How long ago the agent entered its current state, when its server reports that.
    pub(super) age: Option<String>,
    pub(super) status: crate::api::schema::AgentStatus,
    pub(super) focused: bool,
    pub(super) rows: Vec<Vec<crate::ui::ResolvedToken>>,
}

impl AgentRow {
    /// Drops rows that only repeat the space name, for lists where a group header or the
    /// selected chip already names the space.
    pub(super) fn drop_space_only_rows(&mut self) {
        self.rows.retain(|row| {
            !row.iter()
                .all(|token| matches!(token.kind, crate::ui::ResolvedTokenKind::Workspace(_)))
        });
    }
}

/// What decides the Agents list order for one machine's snapshot.
#[derive(Clone, Copy)]
pub(super) struct AgentListOrder<'a> {
    pub(super) sort: crate::config::AgentPanelSortConfig,
    /// Space name a live filter keeps.
    pub(super) space: Option<&'a str>,
    /// Folded space names. Their agents leave the list wherever spaces are shown as groups.
    pub(super) collapsed: Option<&'a HashSet<String>>,
    /// Whether the tree sidebar arranges the list.
    pub(super) tree: bool,
}

/// The parts of the list order that only a single machine's list has.
#[derive(Clone, Copy)]
pub(super) struct LocalListOrder<'a> {
    pub(super) collapsed: &'a HashSet<String>,
    pub(super) tree: bool,
}

impl ClientShellConfig {
    /// Folding and the tree arrangement, when either changes the list a single machine shows.
    pub(super) fn local_list_order(&self) -> Option<LocalListOrder<'_>> {
        (self.sidebar_tree || !self.collapsed_spaces.is_empty()).then_some(LocalListOrder {
            collapsed: &self.collapsed_spaces,
            tree: self.sidebar_tree,
        })
    }

    pub(super) fn agent_list_order<'a>(
        &'a self,
        endpoint_id: &ClientEndpointId,
        snapshot: &ClientShellSnapshot,
    ) -> AgentListOrder<'a> {
        AgentListOrder {
            sort: self.agent_panel_sort,
            space: self.agent_filter_space(endpoint_id, snapshot),
            collapsed: Some(&self.collapsed_spaces),
            tree: self.sidebar_tree,
        }
    }
}

/// Agent panes in the order the Agents list shows them.
pub(super) fn ordered_agent_pane_ids(
    snapshot: &ClientShellSnapshot,
    order: AgentListOrder<'_>,
) -> Vec<String> {
    // Space names are only needed to match a filter or to keep same-named spaces together.
    let gathers = order.sort == crate::config::AgentPanelSortConfig::Spaces
        && snapshot.agent_view_label.is_none()
        && snapshot.agents.len() > 1;
    let spaces = if order.space.is_some() || order.tree || gathers {
        super::agent_filter::space_labels(snapshot)
    } else {
        Vec::new()
    };
    ordered_agent_pane_ids_among(snapshot, &spaces, order)
}

/// [`ordered_agent_pane_ids`] for a caller that already resolved the snapshot's space names.
pub(super) fn ordered_agent_pane_ids_among(
    snapshot: &ClientShellSnapshot,
    spaces: &[(&ClientShellWorkspace, &str)],
    order: AgentListOrder<'_>,
) -> Vec<String> {
    let name = |agent: &crate::protocol::ClientShellAgent| {
        super::agent_filter::space_name(spaces, &agent.workspace_id).to_owned()
    };
    let kept = |agent: &&crate::protocol::ClientShellAgent| {
        order.space.is_none_or(|space| {
            super::agent_filter::space_name(spaces, &agent.workspace_id) == space
        })
    };
    let viewed = snapshot.agent_view_label.is_some();
    let mut agents = if viewed {
        snapshot
            .agent_order
            .iter()
            .filter_map(|pane_id| {
                snapshot
                    .agents
                    .iter()
                    .find(|agent| agent.pane_id == pane_id.as_str())
            })
            .filter(kept)
            .collect::<Vec<_>>()
    } else {
        snapshot.agents.iter().filter(kept).collect::<Vec<_>>()
    };
    let folded = |agents: &mut Vec<&crate::protocol::ClientShellAgent>| {
        if let Some(collapsed) = order.collapsed.filter(|collapsed| !collapsed.is_empty()) {
            agents.retain(|agent| !collapsed.contains(&name(agent)));
        }
    };
    if order.tree {
        // The tree lists spaces in Spaces-panel order and, inside each, the agents that need
        // the user first. A folded space hides its agents from the list and from navigation.
        folded(&mut agents);
        let rank = |agent: &crate::protocol::ClientShellAgent| {
            let space = super::agent_filter::space_name(spaces, &agent.workspace_id);
            spaces
                .iter()
                .position(|(_, name)| *name == space)
                .unwrap_or(usize::MAX)
        };
        agents.sort_by_key(|agent| {
            (
                rank(agent),
                std::cmp::Reverse(status_priority(agent.agent_status)),
            )
        });
    } else if !viewed {
        match order.sort {
            crate::config::AgentPanelSortConfig::Priority => agents.sort_by_key(|agent| {
                (
                    std::cmp::Reverse(status_priority(agent.agent_status)),
                    std::cmp::Reverse(agent.state_change_seq),
                )
            }),
            crate::config::AgentPanelSortConfig::Spaces if order.space.is_none() => {
                let ranks = super::agent_groups::first_appearance_ranks(
                    agents.iter().map(|agent| name(agent)),
                );
                super::agent_groups::gather_by_rank(&mut agents, ranks);
                // Folding is offered on the group headers, which need two spaces to appear.
                if !super::agent_groups::group_starts(agents.iter().map(|agent| name(agent)))
                    .is_empty()
                {
                    folded(&mut agents);
                }
            }
            crate::config::AgentPanelSortConfig::Spaces => {}
        }
    }
    agents
        .into_iter()
        .map(|agent| agent.pane_id.clone())
        .collect()
}

pub(super) fn render_agent_panel(
    buffer: &mut Buffer,
    area: Rect,
    endpoint_id: &ClientEndpointId,
    snapshot: &ClientShellSnapshot,
    config: &ClientShellConfig,
    agent_scroll: &mut usize,
    reveal_pane: Option<String>,
    hits: &mut ShellHitMap,
) {
    if !render_agent_panel_header(
        buffer,
        area,
        snapshot.agent_view_label.as_deref(),
        config,
        hits,
    ) {
        return;
    }

    let now_ms = crate::agent_state_time::unix_ms_now();
    // Resolved once per frame for the filter, the chips, the order, and the headers.
    let spaces = if snapshot.agents.is_empty() && config.agent_panel_filter.is_none() {
        Vec::new()
    } else {
        super::agent_filter::space_labels(snapshot)
    };
    let filter = config
        .agent_panel_filter
        .as_ref()
        .filter(|filter| filter.space_among(endpoint_id, &spaces).is_some());
    let list_offset = super::agent_filter::render_chips(
        buffer,
        area,
        super::agent_filter::AGENT_PANEL_TITLE_ROWS,
        || super::agent_filter::snapshot_chips(endpoint_id, snapshot, &spaces, filter),
        filter,
        config,
        hits,
    );
    // Every agent the list could show; folding is applied to the items so a folded space can
    // still say how many agents it holds.
    let mut rows = ordered_agent_pane_ids_among(
        snapshot,
        &spaces,
        AgentListOrder {
            sort: config.agent_panel_sort,
            space: filter.map(|filter| filter.space.as_str()),
            collapsed: None,
            tree: false,
        },
    )
    .into_iter()
    .filter_map(|pane_id| agent_row(snapshot, &pane_id, config, None, now_ms))
    .collect::<Vec<_>>();
    let name =
        |row: &AgentRow| super::agent_filter::space_name(&spaces, &row.workspace_id).to_owned();
    let starts = if filter.is_none()
        && super::agent_groups::grouped_order(config, snapshot.agent_view_label.as_deref())
    {
        super::agent_groups::group_starts(rows.iter().map(name))
    } else {
        Vec::new()
    };
    if filter.is_some() || !starts.is_empty() {
        rows.iter_mut().for_each(AgentRow::drop_space_only_rows);
    }
    let items = super::agent_groups::grouped_items(rows, &starts, |row, agents| {
        super::agent_groups::AgentGroupHeader {
            label: name(row),
            target: super::agent_filter::AgentPanelFilter {
                endpoint_id: endpoint_id.clone(),
                space: name(row),
            },
            fold: Some(super::agent_groups::GroupFold {
                collapsed: config.collapsed_spaces.contains(&name(row)),
                agents,
            }),
        }
    });
    let reveal = reveal_pane.and_then(|pane_id| {
        items
            .iter()
            .position(|item| item.row.as_ref().is_some_and(|row| row.pane_id == pane_id))
    });
    render_agent_list(
        buffer,
        area,
        list_offset,
        &items,
        reveal,
        (snapshot.agent_view_label.is_some() || filter.is_some()).then_some(" no matching agents"),
        config,
        agent_scroll,
        hits,
        |row| row.rows.len(),
        |buffer, rect, row, hits| {
            hits.agents.push((rect, row.pane_id.clone()));
            hits.agent_ages_shown |= render_agent_row(buffer, rect, row, config);
        },
    );
}

pub(super) fn render_agent_panel_header(
    buffer: &mut Buffer,
    area: Rect,
    agent_view_label: Option<&str>,
    config: &ClientShellConfig,
    hits: &mut ShellHitMap,
) -> bool {
    if area.height == 0 {
        return false;
    }
    put_text(
        buffer,
        area.x,
        area.y,
        area.width,
        &"─".repeat(area.width as usize),
        Style::default().fg(config.palette.surface_dim),
    );
    if area.height < 2 {
        return false;
    }
    put_text(
        buffer,
        area.x,
        area.y + 1,
        area.width,
        " agents",
        Style::default()
            .fg(config.palette.overlay0)
            .add_modifier(Modifier::BOLD),
    );
    let sort_label = agent_view_label.unwrap_or(match config.agent_panel_sort {
        crate::config::AgentPanelSortConfig::Spaces => "grouped",
        crate::config::AgentPanelSortConfig::Priority => "priority",
    });
    let sort_width = display_width(sort_label).min(area.width as usize) as u16;
    let sort_rect = Rect::new(
        area.right().saturating_sub(sort_width),
        area.y + 1,
        sort_width,
        1,
    );
    hits.agent_sort_toggle = if config.mouse_capture && agent_view_label.is_none() {
        sort_rect
    } else {
        Rect::default()
    };
    put_text(
        buffer,
        sort_rect.x,
        sort_rect.y,
        sort_rect.width,
        sort_label,
        Style::default()
            .fg(if agent_view_label.is_some() {
                config.palette.accent
            } else {
                config.palette.overlay0
            })
            .add_modifier(Modifier::BOLD),
    );
    true
}

/// Draws the Agents list. `reveal` names the item to scroll into view first.
pub(super) fn render_agent_list<T>(
    buffer: &mut Buffer,
    area: Rect,
    list_offset: u16,
    items: &[super::agent_groups::AgentListItem<T>],
    reveal: Option<usize>,
    empty_message: Option<&str>,
    config: &ClientShellConfig,
    agent_scroll: &mut usize,
    hits: &mut ShellHitMap,
    row_lines: impl Fn(&T) -> usize,
    mut render_row: impl FnMut(&mut Buffer, Rect, &T, &mut ShellHitMap),
) {
    let body = Rect::new(
        area.x,
        area.y.saturating_add(list_offset),
        area.width,
        area.height.saturating_sub(list_offset),
    );
    hits.agent_body = body;
    if body.is_empty() || items.is_empty() {
        *agent_scroll = 0;
        if let Some(message) = empty_message.filter(|_| !body.is_empty()) {
            put_text(
                buffer,
                body.x,
                body.y,
                body.width,
                message,
                Style::default()
                    .fg(config.palette.overlay0)
                    .add_modifier(Modifier::DIM),
            );
        }
        return;
    }

    let (heights, gaps) = super::agent_groups::list_layout(
        items
            .iter()
            .map(|item| (item.header.is_some(), item.row.as_ref().map(&row_lines))),
        config.agents.row_gap,
    );
    if let Some(target) = reveal {
        *agent_scroll = super::scroll::list_scroll_start_to_reveal(
            &heights,
            &gaps,
            body.height,
            *agent_scroll,
            target,
        );
    }
    let metrics = super::scroll::list_scroll_metrics(&heights, &gaps, body.height, *agent_scroll);
    hits.agent_max_scroll = metrics.max_offset_from_bottom;
    hits.agent_scroll_metrics = Some(metrics);
    *agent_scroll = metrics
        .max_offset_from_bottom
        .saturating_sub(metrics.offset_from_bottom);
    let show_scrollbar = metrics.max_offset_from_bottom > 0 && body.width > 1;
    let content_width = body.width.saturating_sub(u16::from(show_scrollbar));
    let mut y = body.y;
    for (index, item) in items.iter().enumerate().skip(*agent_scroll) {
        let height = heights[index].min(body.height);
        if y.saturating_add(height) > body.bottom() {
            break;
        }
        // A header gives way before the row it sits on when the list is one line tall.
        let header_rows = u16::from(item.header.is_some())
            .min(height.saturating_sub(u16::from(item.row.is_some())));
        if let Some(header) = item.header.as_ref().filter(|_| header_rows > 0) {
            render_group_header(
                buffer,
                Rect::new(body.x, y, content_width, 1),
                header,
                config,
                hits,
            );
        }
        if let Some(row) = &item.row {
            let rect = Rect::new(body.x, y + header_rows, content_width, height - header_rows);
            render_row(buffer, rect, row, hits);
        }
        y = y.saturating_add(height).saturating_add(gaps[index]);
    }

    if show_scrollbar {
        let track = Rect::new(body.right().saturating_sub(1), body.y, 1, body.height);
        hits.agent_scrollbar = track;
        super::scroll::render_list_scrollbar(buffer, track, metrics, &config.palette);
    }
}

fn render_group_header(
    buffer: &mut Buffer,
    rect: Rect,
    header: &super::agent_groups::AgentGroupHeader,
    config: &ClientShellConfig,
    hits: &mut ShellHitMap,
) {
    // A folded space says how many agents it hides, next to the control that shows them.
    let control = header.fold.map(|fold| {
        if fold.collapsed {
            format!("{} ▸", fold.agents)
        } else {
            "▾".to_owned()
        }
    });
    let control_width = control
        .as_deref()
        .map_or(0, |control| display_width(control) as u16 + 1)
        .min(rect.width);
    put_text(
        buffer,
        rect.x.saturating_add(1),
        rect.y,
        rect.width.saturating_sub(1 + control_width),
        &header.label,
        Style::default()
            .fg(config.palette.overlay0)
            .add_modifier(Modifier::BOLD),
    );
    if config.mouse_capture {
        hits.agent_group_headers.push((rect, header.target.clone()));
    }
    let Some(control) = control else {
        return;
    };
    let width = (display_width(&control) as u16).min(rect.width);
    put_text(
        buffer,
        rect.right() - width,
        rect.y,
        width,
        &control,
        Style::default().fg(config.palette.accent),
    );
    if config.mouse_capture {
        hits.space_collapse_toggles.push((
            Rect::new(rect.right() - control_width, rect.y, control_width, 1),
            header.target.space.clone(),
        ));
    }
}

pub(super) fn agent_row(
    snapshot: &ClientShellSnapshot,
    pane_id: &str,
    config: &ClientShellConfig,
    machine: Option<&str>,
    now_ms: u64,
) -> Option<AgentRow> {
    let agent = snapshot
        .agents
        .iter()
        .find(|agent| agent.pane_id == pane_id)?;
    let workspace = snapshot
        .workspaces
        .iter()
        .find(|workspace| workspace.workspace_id == agent.workspace_id)?;
    let tab = snapshot.tabs.iter().find(|tab| tab.tab_id == agent.tab_id);
    let pane = snapshot
        .panes
        .iter()
        .find(|pane| pane.pane_id == agent.pane_id);
    let tab_count = snapshot
        .tabs
        .iter()
        .filter(|candidate| candidate.workspace_id == agent.workspace_id)
        .count();
    let tab_label = tab
        .filter(|tab| tab_count > 1 || tab.custom_label)
        .map(|tab| tab.label.as_str());
    let agent_label = agent
        .display_agent
        .as_deref()
        .or(agent.name.as_deref())
        .or(agent.agent.as_deref())
        .or(agent.title.as_deref());
    let labels = agent
        .state_labels
        .iter()
        .cloned()
        .collect::<HashMap<_, _>>();
    let tokens = agent.tokens.iter().cloned().collect::<HashMap<_, _>>();
    let state_text = labels
        .get(status_text(agent.agent_status))
        .map(String::as_str)
        .unwrap_or_else(|| sidebar_status_text(agent.agent_status));
    let canonical_agent = agent
        .agent
        .as_deref()
        .and_then(crate::detect::parse_agent_label);
    let rows = crate::ui::sidebar_agent_rows(
        &config.agents,
        crate::ui::AgentTokenContext {
            machine,
            workspace: &workspace.label,
            tab: tab_label,
            pane: agent
                .title
                .as_deref()
                .or_else(|| pane.and_then(|pane| pane.label.as_deref())),
            agent_label,
            terminal_title: agent.terminal_title.as_deref(),
            terminal_title_stripped: agent.terminal_title_stripped.as_deref(),
            canonical_agent,
            tokens: &tokens,
        },
        state_text,
    );
    Some(AgentRow {
        pane_id: agent.pane_id.clone(),
        workspace_id: agent.workspace_id.clone(),
        age: crate::agent_state_time::changed_at_ms(
            agent
                .tokens
                .iter()
                .map(|(name, value)| (name.as_str(), value.as_str())),
        )
        .map(|changed_at| crate::agent_state_time::age_label(changed_at, now_ms)),
        status: agent.agent_status,
        focused: agent.focused,
        rows,
    })
}

/// Cells an agent's first row keeps for its text before the age is dropped to make room.
const AGE_MIN_TEXT_WIDTH: u16 = 8;

/// Draws one agent entry and reports whether its age was shown.
pub(super) fn render_agent_row(
    buffer: &mut Buffer,
    rect: Rect,
    row: &AgentRow,
    config: &ClientShellConfig,
) -> bool {
    let palette = &config.palette;
    let row_style = if row.focused {
        Style::default().bg(palette.active_row_bg)
    } else {
        Style::default()
    };
    let name_style = if row.focused {
        Style::default()
            .fg(palette.text)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(palette.subtext0)
            .add_modifier(Modifier::BOLD)
    };
    let status_style = Style::default().fg(status_color(row.status, palette));
    let secondary = Style::default().fg(palette.overlay0);
    let icon = (
        status_icon(row.status, config.status_indicators),
        Style::default().fg(status_color(row.status, palette)),
    );
    let rows = if row.rows.is_empty() {
        vec![vec![crate::ui::ResolvedToken {
            kind: crate::ui::ResolvedTokenKind::StateIcon,
            style: Default::default(),
        }]]
    } else {
        row.rows.clone()
    };
    // The age sits at the right edge of the first row, after a one-cell gap.
    let age = row.age.as_deref().filter(|age| {
        let reserved = display_width(age) as u16 + 1;
        rect.height > 0 && rect.width >= 1 + reserved + AGE_MIN_TEXT_WIDTH
    });
    for (index, tokens) in rows.iter().take(rect.height as usize).enumerate() {
        let indent = if index == 0 { 1 } else { 3 };
        let reserved = age
            .filter(|_| index == 0)
            .map_or(0, |age| display_width(age) as u16 + 1);
        let mut spans = vec![ratatui::text::Span::raw(" ".repeat(indent))];
        spans.extend(crate::ui::resolved_token_spans(
            tokens,
            icon,
            status_style,
            name_style,
            secondary,
            secondary,
            palette,
            rect.width.saturating_sub(indent as u16 + reserved) as usize,
        ));
        Paragraph::new(Line::from(spans)).style(row_style).render(
            Rect::new(rect.x, rect.y + index as u16, rect.width, 1),
            buffer,
        );
    }
    if let Some(age) = age {
        let width = display_width(age) as u16;
        put_text(
            buffer,
            rect.right().saturating_sub(width),
            rect.y,
            width,
            age,
            secondary,
        );
    }
    age.is_some()
}

fn put_text(buffer: &mut Buffer, x: u16, y: u16, width: u16, text: &str, style: Style) {
    for (offset, character) in text.chars().take(width as usize).enumerate() {
        if let Some(cell) = buffer.cell_mut((x + offset as u16, y)) {
            cell.set_char(character).set_style(style);
        }
    }
}

fn display_width(text: &str) -> usize {
    unicode_width::UnicodeWidthStr::width(text)
}

fn sidebar_status_text(status: crate::api::schema::AgentStatus) -> &'static str {
    use crate::api::schema::AgentStatus;
    match status {
        AgentStatus::Blocked => "blocked",
        AgentStatus::Done => "done",
        AgentStatus::Working => "working",
        AgentStatus::Idle | AgentStatus::Unknown => "idle",
    }
}
