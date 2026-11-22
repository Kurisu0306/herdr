use super::render::put_text;
use super::*;

pub(super) fn render_collapsed(
    buffer: &mut Buffer,
    area: Rect,
    endpoints: &[ClientShellEndpoint],
    active_endpoint_id: &ClientEndpointId,
    config: &ClientShellConfig,
    hits: &mut ShellHitMap,
) {
    let rows = agent_rows(endpoints, active_endpoint_id, config);
    for (index, row) in rows.into_iter().take(area.height as usize).enumerate() {
        let rect = Rect::new(area.x, area.y + index as u16, area.width, 1);
        if row.agent.focused {
            buffer.set_style(rect, Style::default().bg(config.palette.active_row_bg));
        }
        let initial = row.machine_label.chars().next().unwrap_or('?');
        put_text(
            buffer,
            rect.x,
            rect.y,
            rect.width,
            &format!(
                "{initial}{}",
                status_icon(row.agent.status, config.status_indicators)
            ),
            Style::default()
                .fg(if row.stale {
                    config.palette.overlay0
                } else {
                    status_color(row.agent.status, &config.palette)
                })
                .add_modifier(if row.stale {
                    Modifier::DIM
                } else {
                    Modifier::empty()
                }),
        );
        hits.endpoint_agents
            .push((rect, row.endpoint_id, row.agent.pane_id));
    }
}

pub(super) fn render_expanded(
    buffer: &mut Buffer,
    area: Rect,
    agent_view_label: Option<&str>,
    endpoints: &[ClientShellEndpoint],
    active_endpoint_id: &ClientEndpointId,
    config: &ClientShellConfig,
    agent_scroll: &mut usize,
    hits: &mut ShellHitMap,
) {
    if !super::agent_sidebar::render_agent_panel_header(
        buffer,
        area,
        agent_view_label,
        config,
        hits,
    ) {
        return;
    }
    let filter = super::agent_filter::live_filter(config.agent_panel_filter.as_ref(), endpoints);
    let list_offset = super::agent_filter::render_chips(
        buffer,
        area,
        super::agent_filter::AGENT_PANEL_TITLE_ROWS,
        || super::agent_filter::endpoint_chips(endpoints, filter),
        filter,
        config,
        hits,
    );
    let items = agent_list(endpoints, active_endpoint_id, config, agent_view_label);
    super::agent_sidebar::render_agent_list(
        buffer,
        area,
        list_offset,
        &items,
        None,
        (agent_view_label.is_some() || filter.is_some()).then_some(" no matching agents"),
        config,
        agent_scroll,
        hits,
        |row| row.agent.rows.len(),
        |buffer, rect, row, hits| {
            hits.agent_ages_shown |=
                super::agent_sidebar::render_agent_row(buffer, rect, &row.agent, config);
            if row.stale {
                buffer.set_style(
                    rect,
                    Style::default()
                        .fg(config.palette.overlay0)
                        .add_modifier(Modifier::DIM),
                );
            }
            hits.endpoint_agents
                .push((rect, row.endpoint_id.clone(), row.agent.pane_id.clone()));
        },
    );
}

impl ClientShellState {
    pub(super) fn reveal_endpoint_agent(
        &mut self,
        endpoint_id: &ClientEndpointId,
        pane_id: &str,
        body_height: u16,
    ) {
        if body_height == 0 {
            return;
        }
        let items = agent_list(
            &self.endpoints,
            &self.active_endpoint_id,
            &self.config,
            self.snapshot
                .as_deref()
                .and_then(|snapshot| snapshot.agent_view_label.as_deref()),
        );
        let Some(target) = items.iter().position(|item| {
            item.row
                .as_ref()
                .is_some_and(|row| &row.endpoint_id == endpoint_id && row.agent.pane_id == pane_id)
        }) else {
            return;
        };
        let (heights, gaps) = super::agent_groups::list_layout(
            items.iter().map(|item| {
                (
                    item.header.is_some(),
                    item.row.as_ref().map(|row| row.agent.rows.len()),
                )
            }),
            self.config.agents.row_gap,
        );
        self.agent_scroll = super::scroll::list_scroll_start_to_reveal(
            &heights,
            &gaps,
            body_height,
            self.agent_scroll,
            target,
        );
    }
}

struct EndpointAgentRow {
    endpoint_id: ClientEndpointId,
    machine_label: String,
    stale: bool,
    agent: super::agent_sidebar::AgentRow,
}

/// Items of the expanded list: rows under the space headers they are drawn with. Rendering and
/// reveal both use this, so scroll positions are computed from the geometry that is drawn.
fn agent_list(
    endpoints: &[ClientShellEndpoint],
    active_endpoint_id: &ClientEndpointId,
    config: &ClientShellConfig,
    agent_view_label: Option<&str>,
) -> Vec<super::agent_groups::AgentListItem<EndpointAgentRow>> {
    let mut rows = agent_rows(endpoints, active_endpoint_id, config);
    let filtered =
        super::agent_filter::live_filter(config.agent_panel_filter.as_ref(), endpoints).is_some();
    let spaces = endpoints
        .iter()
        .filter_map(|endpoint| {
            let snapshot = endpoint.snapshot.as_deref()?;
            Some((
                &endpoint.endpoint_id,
                super::agent_filter::space_labels(snapshot),
            ))
        })
        .collect::<Vec<_>>();
    let name = |row: &EndpointAgentRow| {
        spaces
            .iter()
            .find(|(endpoint_id, _)| **endpoint_id == row.endpoint_id)
            .map_or(row.agent.workspace_id.as_str(), |(_, spaces)| {
                super::agent_filter::space_name(spaces, &row.agent.workspace_id)
            })
            .to_owned()
    };
    let starts = if !filtered && super::agent_groups::grouped_order(config, agent_view_label) {
        super::agent_groups::group_starts(rows.iter().map(|row| (&row.endpoint_id, name(row))))
    } else {
        Vec::new()
    };
    if filtered || !starts.is_empty() {
        rows.iter_mut()
            .for_each(|row| row.agent.drop_space_only_rows());
    }
    super::agent_groups::grouped_items(rows, &starts, |row, _| {
        super::agent_groups::AgentGroupHeader {
            // Spaces on different machines can share a name.
            label: format!("{} · {}", row.machine_label, name(row)),
            target: super::agent_filter::AgentPanelFilter {
                endpoint_id: row.endpoint_id.clone(),
                space: name(row),
            },
            // Folded space names are kept per machine; a list across machines offers none.
            fold: None,
        }
    })
}

fn agent_rows(
    endpoints: &[ClientShellEndpoint],
    active_endpoint_id: &ClientEndpointId,
    config: &ClientShellConfig,
) -> Vec<EndpointAgentRow> {
    let now_ms = crate::agent_state_time::unix_ms_now();
    let mut rendered_rows = endpoints
        .iter()
        .filter_map(|endpoint| {
            endpoint.snapshot.as_deref().map(|snapshot| {
                snapshot
                    .agents
                    .iter()
                    .filter_map(|agent| {
                        super::agent_sidebar::agent_row(
                            snapshot,
                            &agent.pane_id,
                            config,
                            Some(&endpoint.label),
                            now_ms,
                        )
                    })
                    .map(|agent| ((endpoint.endpoint_id.clone(), agent.pane_id.clone()), agent))
                    .collect::<Vec<_>>()
            })
        })
        .flatten()
        .collect::<HashMap<_, _>>();

    super::aggregate_navigation::aggregate_agent_rows(
        endpoints,
        active_endpoint_id,
        config.agent_panel_sort,
        config.agent_panel_filter.as_ref(),
        config.local_list_order(),
    )
    .into_iter()
    .filter_map(|row| {
        let key = (row.endpoint.endpoint_id.clone(), row.agent.pane_id.clone());
        let mut agent = rendered_rows.remove(&key)?;
        agent.focused &= row.endpoint.endpoint_id == active_endpoint_id;
        Some(EndpointAgentRow {
            endpoint_id: row.endpoint.endpoint_id.clone(),
            machine_label: row.endpoint.label.to_owned(),
            stale: row.endpoint.stale(),
            agent,
        })
    })
    .collect()
}
