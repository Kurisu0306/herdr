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
        || super::agent_filter::endpoint_chips(endpoints, filter),
        filter,
        config,
        hits,
    );
    let (rows, headers) = agent_list(endpoints, active_endpoint_id, config, agent_view_label);
    super::agent_sidebar::render_agent_list(
        buffer,
        area,
        list_offset,
        &rows,
        &headers,
        (agent_view_label.is_some() || filter.is_some()).then_some(" no matching agents"),
        config,
        agent_scroll,
        hits,
        |row| row.agent.rows.len(),
        |buffer, rect, row, hits| {
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
        let (rows, headers) = agent_list(
            &self.endpoints,
            &self.active_endpoint_id,
            &self.config,
            self.snapshot
                .as_deref()
                .and_then(|snapshot| snapshot.agent_view_label.as_deref()),
        );
        let Some(target) = rows
            .iter()
            .position(|row| &row.endpoint_id == endpoint_id && row.agent.pane_id == pane_id)
        else {
            return;
        };
        let (heights, gaps) = super::agent_groups::list_layout(
            rows.iter().map(|row| row.agent.rows.len()),
            &headers,
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

/// Rows of the expanded list with the space headers they are drawn under. Rendering and reveal
/// both use this, so scroll positions are computed from the geometry that is drawn.
fn agent_list(
    endpoints: &[ClientShellEndpoint],
    active_endpoint_id: &ClientEndpointId,
    config: &ClientShellConfig,
    agent_view_label: Option<&str>,
) -> (
    Vec<EndpointAgentRow>,
    Vec<Option<super::agent_groups::AgentGroupHeader>>,
) {
    let mut rows = agent_rows(endpoints, active_endpoint_id, config);
    let filtered =
        super::agent_filter::live_filter(config.agent_panel_filter.as_ref(), endpoints).is_some();
    let headers = if !filtered && super::agent_groups::grouped_order(config, agent_view_label) {
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
        let starts =
            super::agent_groups::group_starts(rows.iter().map(|row| (&row.endpoint_id, name(row))));
        rows.iter()
            .zip(starts)
            .map(|(row, start)| {
                start.then(|| super::agent_groups::AgentGroupHeader {
                    // Spaces on different machines can share a name.
                    label: format!("{} · {}", row.machine_label, name(row)),
                    target: super::agent_filter::AgentPanelFilter {
                        endpoint_id: row.endpoint_id.clone(),
                        space: name(row),
                    },
                })
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    if filtered || !headers.is_empty() {
        rows.iter_mut()
            .for_each(|row| row.agent.drop_space_only_rows());
    }
    (rows, headers)
}

fn agent_rows(
    endpoints: &[ClientShellEndpoint],
    active_endpoint_id: &ClientEndpointId,
    config: &ClientShellConfig,
) -> Vec<EndpointAgentRow> {
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
