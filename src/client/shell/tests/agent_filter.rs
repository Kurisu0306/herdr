use super::*;

fn two_space_snapshot() -> ClientShellSnapshot {
    let mut projected = snapshot();
    let mut second = projected.workspaces[0].clone();
    second.workspace_id = "ws_2".into();
    second.active_tab_id = "tab_2".into();
    second.number = 2;
    second.label = "second".into();
    second.focused = false;
    projected.workspaces.push(second);
    let mut second_tab = projected.tabs[0].clone();
    second_tab.tab_id = "tab_2".into();
    second_tab.workspace_id = "ws_2".into();
    second_tab.focused = false;
    projected.tabs.push(second_tab);
    let mut second_pane = projected.panes[0].clone();
    second_pane.pane_id = "pane_2".into();
    second_pane.workspace_id = "ws_2".into();
    second_pane.tab_id = "tab_2".into();
    second_pane.focused = false;
    projected.panes.push(second_pane);
    projected.agents = vec![
        space_agent("pane_1", "ws_1", "tab_1", true),
        space_agent("pane_2", "ws_2", "tab_2", false),
    ];
    projected
}

fn space_agent(pane_id: &str, workspace_id: &str, tab_id: &str, focused: bool) -> ClientShellAgent {
    ClientShellAgent {
        pane_id: pane_id.into(),
        workspace_id: workspace_id.into(),
        tab_id: tab_id.into(),
        name: Some(pane_id.into()),
        display_agent: None,
        agent: Some("pi".into()),
        title: None,
        terminal_title: None,
        terminal_title_stripped: None,
        agent_status: AgentStatus::Idle,
        state_change_seq: 1,
        state_labels: Vec::new(),
        tokens: Vec::new(),
        focused,
    }
}

fn two_space_state() -> ClientShellState {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(two_space_snapshot()));
    state.set_pane_surface(surface());
    state.compose(106, 30).expect("agent sidebar frame");
    state
}

fn listed_agents(state: &ClientShellState) -> Vec<&str> {
    state
        .hits
        .agents
        .iter()
        .map(|(_, pane_id)| pane_id.as_str())
        .collect()
}

fn chip_rect(state: &ClientShellState, workspace_id: Option<&str>) -> Rect {
    state
        .hits
        .agent_filter_chips
        .iter()
        .find(|(_, target)| {
            target.as_ref().map(|target| target.workspace_id.as_str()) == workspace_id
        })
        .map(|(rect, _)| *rect)
        .expect("space chip")
}

fn click(state: &mut ClientShellState, rect: Rect) -> ClientShellInput {
    let outcome = state.handle_raw_events(vec![RawInputEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: rect.x,
        row: rect.y,
        modifiers: KeyModifiers::empty(),
    })]);
    state.compose(106, 30).expect("agent sidebar frame");
    outcome
}

#[test]
fn space_chip_narrows_the_agent_list_without_asking_the_server() {
    let mut state = two_space_state();
    assert_eq!(state.hits.agent_filter_chips.len(), 3);
    assert_eq!(listed_agents(&state), ["pane_1", "pane_2"]);

    let chip = chip_rect(&state, Some("ws_2"));
    let outcome = click(&mut state, chip);

    assert!(outcome.repaint);
    assert!(outcome.actions.is_empty() && outcome.requests.is_empty());
    assert_eq!(listed_agents(&state), ["pane_2"]);
}

#[test]
fn selected_chip_and_all_chip_both_restore_every_space() {
    for restore in [Some("ws_2"), None] {
        let mut state = two_space_state();
        let chip = chip_rect(&state, Some("ws_2"));
        click(&mut state, chip);
        assert_eq!(listed_agents(&state), ["pane_2"]);

        let chip = chip_rect(&state, restore);
        click(&mut state, chip);

        assert_eq!(state.config.agent_panel_filter, None);
        assert_eq!(listed_agents(&state), ["pane_1", "pane_2"]);
    }
}

#[test]
fn keyboard_agent_navigation_follows_the_filtered_list() {
    let mut state = two_space_state();
    let chip = chip_rect(&state, Some("ws_2"));
    click(&mut state, chip);

    let mut first = ClientShellInput::default();
    state.record_binding(
        crate::input::KeybindMatch::Action(crate::input::KeybindAction::FocusAgent(0)),
        &mut first,
    );
    assert!(matches!(
        &first.actions[..],
        [ClientShellAction::Endpoint { request, .. }]
            if matches!(
                &request.method,
                crate::api::schema::Method::PaneFocus(target) if target.pane_id == "pane_2"
            )
    ));

    // An index past the filtered list must not reach the server's unfiltered order.
    let mut second = ClientShellInput::default();
    state.record_binding(
        crate::input::KeybindMatch::Action(crate::input::KeybindAction::FocusAgent(1)),
        &mut second,
    );
    assert!(second.actions.is_empty());
}

#[test]
fn collapsed_sidebar_lists_only_the_selected_space() {
    let mut state = two_space_state();
    let chip = chip_rect(&state, Some("ws_2"));
    click(&mut state, chip);

    state.sidebar_collapsed = true;
    state.compose(106, 30).expect("collapsed sidebar frame");

    assert_eq!(listed_agents(&state), ["pane_2"]);
}

#[test]
fn filter_for_a_closed_space_shows_every_agent() {
    let mut remaining = two_space_snapshot();
    remaining.workspaces.truncate(1);
    remaining.tabs.truncate(1);
    remaining.panes.truncate(1);
    remaining.agents.truncate(1);
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.config.agent_panel_filter = Some(super::super::agent_filter::AgentPanelFilter {
        endpoint_id: ClientEndpointId::Local,
        workspace_id: "ws_2".into(),
    });
    state.set_snapshot(Box::new(remaining));
    state.set_pane_surface(surface());
    state.compose(106, 30).expect("agent sidebar frame");

    assert_eq!(listed_agents(&state), ["pane_1"]);
    assert!(state.hits.agent_filter_chips.is_empty());
}

#[test]
fn single_space_keeps_the_original_agent_list_position() {
    let mut projected = snapshot();
    projected.agents = vec![space_agent("pane_1", "ws_1", "tab_1", true)];
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(projected));
    state.set_pane_surface(surface());
    state.compose(106, 30).expect("agent sidebar frame");
    let single_space_body = state.hits.agent_body;
    assert!(state.hits.agent_filter_chips.is_empty());

    let two_spaces = two_space_state();

    assert!(two_spaces.hits.agent_body.y > single_space_body.y);
    assert_eq!(
        two_spaces.hits.agent_body.bottom(),
        single_space_body.bottom()
    );
}

#[test]
fn chips_need_the_mouse_unless_a_filter_is_active() {
    let mut state = two_space_state();
    state.config.mouse_capture = false;
    let frame = state.compose(106, 30).expect("agent sidebar frame");
    assert!(state.hits.agent_filter_chips.is_empty());
    assert!(!frame_rows(&frame).iter().any(|row| row.contains(" all ")));

    state.config.agent_panel_filter = Some(super::super::agent_filter::AgentPanelFilter {
        endpoint_id: ClientEndpointId::Local,
        workspace_id: "ws_2".into(),
    });
    let frame = state.compose(106, 30).expect("agent sidebar frame");

    assert!(state.hits.agent_filter_chips.is_empty());
    assert!(frame_rows(&frame)
        .iter()
        .any(|row| row.contains(" second ")));
}

#[test]
fn selected_space_without_agents_keeps_its_chip_and_explains_the_empty_list() {
    let mut projected = two_space_snapshot();
    projected.agents.truncate(1);
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(projected));
    state.set_pane_surface(surface());
    state.compose(106, 30).expect("agent sidebar frame");
    // Only one space has agents, so there is nothing to choose between yet.
    assert!(state.hits.agent_filter_chips.is_empty());

    state.config.agent_panel_filter = Some(super::super::agent_filter::AgentPanelFilter {
        endpoint_id: ClientEndpointId::Local,
        workspace_id: "ws_2".into(),
    });
    let frame = state.compose(106, 30).expect("agent sidebar frame");
    assert!(listed_agents(&state).is_empty());
    assert!(frame_rows(&frame)
        .iter()
        .any(|row| row.contains("no matching agents")));

    let all = chip_rect(&state, None);
    chip_rect(&state, Some("ws_2"));
    click(&mut state, all);

    assert_eq!(listed_agents(&state), ["pane_1"]);
}

fn header_rect(state: &ClientShellState, workspace_id: &str) -> Rect {
    state
        .hits
        .agent_group_headers
        .iter()
        .find(|(_, target)| target.workspace_id == workspace_id)
        .map(|(rect, _)| *rect)
        .expect("space header")
}

fn agent_rect(state: &ClientShellState, pane_id: &str) -> Rect {
    state
        .hits
        .agents
        .iter()
        .find(|(_, listed)| listed == pane_id)
        .map(|(rect, _)| *rect)
        .expect("agent row")
}

fn composed(config: &Config, projected: ClientShellSnapshot) -> ClientShellState {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(config));
    state.set_snapshot(Box::new(projected));
    state.set_pane_surface(surface());
    state.compose(106, 30).expect("agent sidebar frame");
    state
}

#[test]
fn grouped_list_names_each_space_above_its_agents() {
    let mut state = two_space_state();
    let frame = state.compose(106, 30).expect("agent sidebar frame");
    let rows = frame_rows(&frame);

    let first = header_rect(&state, "ws_1");
    let second = header_rect(&state, "ws_2");
    assert_eq!(state.hits.agent_group_headers.len(), 2);
    assert!(rows[first.y as usize].contains(" client-shell"));
    assert!(rows[second.y as usize].contains(" second"));
    assert_eq!(agent_rect(&state, "pane_1").y, first.y + 1);
    assert_eq!(agent_rect(&state, "pane_2").y, second.y + 1);
    // A blank row separates one space's agents from the next header.
    assert_eq!(second.y, agent_rect(&state, "pane_1").bottom() + 1);
}

#[test]
fn clicking_a_space_header_selects_that_space() {
    let mut state = two_space_state();
    let header = header_rect(&state, "ws_2");

    let outcome = click(&mut state, header);

    assert!(outcome.actions.is_empty() && outcome.requests.is_empty());
    assert_eq!(listed_agents(&state), ["pane_2"]);
    // The selected chip names the space now, so the list needs no header.
    assert!(state.hits.agent_group_headers.is_empty());
}

#[test]
fn interleaved_orders_keep_the_flat_list() {
    let mut priority = Config::default();
    priority.ui.agent_panel_sort = crate::config::AgentPanelSortConfig::Priority;
    assert!(composed(&priority, two_space_snapshot())
        .hits
        .agent_group_headers
        .is_empty());

    let mut viewed = two_space_snapshot();
    viewed.agent_view_label = Some("review".into());
    viewed.agent_order = vec!["pane_2".into(), "pane_1".into()];
    let state = composed(&Config::default(), viewed);
    assert_eq!(listed_agents(&state), ["pane_2", "pane_1"]);
    assert!(state.hits.agent_group_headers.is_empty());
}

#[test]
fn one_space_keeps_the_list_without_a_header() {
    let mut projected = two_space_snapshot();
    projected.agents.truncate(1);

    let state = composed(&Config::default(), projected);

    assert!(state.hits.agent_group_headers.is_empty());
    assert_eq!(agent_rect(&state, "pane_1").y, state.hits.agent_body.y);
}

#[test]
fn rows_that_only_repeat_the_space_fold_into_its_header() {
    use crate::config::AgentSidebarToken;

    let mut config = Config::default();
    config.ui.sidebar.agents.rows = vec![
        vec![AgentSidebarToken::StateIcon, AgentSidebarToken::Agent],
        vec![AgentSidebarToken::Workspace],
    ];
    let grouped = composed(&config, two_space_snapshot());
    assert_eq!(agent_rect(&grouped, "pane_1").height, 1);

    // Without headers the row is the only place the space is named, so it stays.
    config.ui.agent_panel_sort = crate::config::AgentPanelSortConfig::Priority;
    let flat = composed(&config, two_space_snapshot());
    assert_eq!(agent_rect(&flat, "pane_1").height, 2);

    // A row that carries more than the space name is never dropped.
    let default_rows = two_space_state();
    assert_eq!(agent_rect(&default_rows, "pane_1").height, 2);
}

#[test]
fn chip_of_a_space_needing_attention_uses_its_status_color() {
    let mut projected = two_space_snapshot();
    projected.workspaces[1].agent_status = AgentStatus::Blocked;
    projected.agents[1].agent_status = AgentStatus::Blocked;
    let mut state = composed(&Config::default(), projected);
    let frame = state.compose(106, 30).expect("agent sidebar frame");
    let buffer = frame.to_ratatui_buffer().expect("buffer");
    let palette = &state.config.palette;

    let blocked = cell_symbol_position(&frame, chip_rect(&state, Some("ws_2")), "second");
    let calm = cell_symbol_position(&frame, chip_rect(&state, Some("ws_1")), "client-shell");

    assert_eq!(
        buffer[blocked].fg,
        super::super::status_color(AgentStatus::Blocked, palette)
    );
    assert_eq!(buffer[calm].fg, palette.overlay0);
}
