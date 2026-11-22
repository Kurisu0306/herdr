use super::*;

pub(super) fn two_space_snapshot() -> ClientShellSnapshot {
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

pub(super) fn space_agent(
    pane_id: &str,
    workspace_id: &str,
    tab_id: &str,
    focused: bool,
) -> ClientShellAgent {
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

pub(super) fn listed_agents(state: &ClientShellState) -> Vec<&str> {
    state
        .hits
        .agents
        .iter()
        .map(|(_, pane_id)| pane_id.as_str())
        .collect()
}

pub(super) fn chip_rect(state: &ClientShellState, space: Option<&str>) -> Rect {
    state
        .hits
        .agent_filter_chips
        .iter()
        .find(|(_, target)| target.as_ref().map(|target| target.space.as_str()) == space)
        .map(|(rect, _)| *rect)
        .expect("space chip")
}

pub(super) fn click(state: &mut ClientShellState, rect: Rect) -> ClientShellInput {
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

    let chip = chip_rect(&state, Some("second"));
    let outcome = click(&mut state, chip);

    assert!(outcome.repaint);
    assert!(outcome.actions.is_empty() && outcome.requests.is_empty());
    assert_eq!(listed_agents(&state), ["pane_2"]);
}

#[test]
fn selected_chip_and_all_chip_both_restore_every_space() {
    for restore in [Some("second"), None] {
        let mut state = two_space_state();
        let chip = chip_rect(&state, Some("second"));
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
    let chip = chip_rect(&state, Some("second"));
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
    let chip = chip_rect(&state, Some("second"));
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
        space: "second".into(),
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
        space: "second".into(),
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
        space: "second".into(),
    });
    let frame = state.compose(106, 30).expect("agent sidebar frame");
    assert!(listed_agents(&state).is_empty());
    assert!(frame_rows(&frame)
        .iter()
        .any(|row| row.contains("no matching agents")));

    let all = chip_rect(&state, None);
    chip_rect(&state, Some("second"));
    click(&mut state, all);

    assert_eq!(listed_agents(&state), ["pane_1"]);
}

fn header_rect(state: &ClientShellState, space: &str) -> Rect {
    state
        .hits
        .agent_group_headers
        .iter()
        .find(|(_, target)| target.space == space)
        .map(|(rect, _)| *rect)
        .expect("space header")
}

pub(super) fn agent_rect(state: &ClientShellState, pane_id: &str) -> Rect {
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

    let first = header_rect(&state, "client-shell");
    let second = header_rect(&state, "second");
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
    let header = header_rect(&state, "second");

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

    let blocked = cell_symbol_position(&frame, chip_rect(&state, Some("second")), "second");
    let calm = cell_symbol_position(
        &frame,
        chip_rect(&state, Some("client-shell")),
        "client-shell",
    );

    assert_eq!(
        buffer[blocked].fg,
        super::super::status_color(AgentStatus::Blocked, palette)
    );
    assert_eq!(buffer[calm].fg, palette.overlay0);
}

/// `client-shell`, `second`, and another `client-shell` opened on the same directory.
pub(super) fn split_project_snapshot() -> ClientShellSnapshot {
    let mut projected = two_space_snapshot();
    let mut third = projected.workspaces[0].clone();
    third.workspace_id = "ws_3".into();
    third.active_tab_id = "tab_3".into();
    third.number = 3;
    third.focused = false;
    projected.workspaces.push(third);
    let mut third_tab = projected.tabs[0].clone();
    third_tab.tab_id = "tab_3".into();
    third_tab.workspace_id = "ws_3".into();
    third_tab.focused = false;
    projected.tabs.push(third_tab);
    let mut third_pane = projected.panes[0].clone();
    third_pane.pane_id = "pane_3".into();
    third_pane.workspace_id = "ws_3".into();
    third_pane.tab_id = "tab_3".into();
    third_pane.focused = false;
    projected.panes.push(third_pane);
    projected
        .agents
        .push(space_agent("pane_3", "ws_3", "tab_3", false));
    projected
}

#[test]
fn same_named_spaces_are_one_group() {
    let mut state = composed(&Config::default(), split_project_snapshot());

    // One chip and one header per name, with the separated workspaces gathered together.
    assert_eq!(state.hits.agent_filter_chips.len(), 3);
    assert_eq!(state.hits.agent_group_headers.len(), 2);
    assert_eq!(listed_agents(&state), ["pane_1", "pane_3", "pane_2"]);
    assert_eq!(
        agent_rect(&state, "pane_3").y,
        agent_rect(&state, "pane_1").bottom()
    );

    let chip = chip_rect(&state, Some("client-shell"));
    click(&mut state, chip);
    assert_eq!(listed_agents(&state), ["pane_1", "pane_3"]);

    let mut second = ClientShellInput::default();
    state.record_binding(
        crate::input::KeybindMatch::Action(crate::input::KeybindAction::FocusAgent(1)),
        &mut second,
    );
    assert!(matches!(
        &second.actions[..],
        [ClientShellAction::Endpoint { request, .. }]
            if matches!(
                &request.method,
                crate::api::schema::Method::PaneFocus(target) if target.pane_id == "pane_3"
            )
    ));
}

#[test]
fn merged_chip_shows_the_most_urgent_space() {
    let mut projected = split_project_snapshot();
    projected.workspaces[2].agent_status = AgentStatus::Blocked;
    projected.agents[2].agent_status = AgentStatus::Blocked;
    let mut state = composed(&Config::default(), projected);
    let frame = state.compose(106, 30).expect("agent sidebar frame");
    let buffer = frame.to_ratatui_buffer().expect("buffer");

    let merged = cell_symbol_position(
        &frame,
        chip_rect(&state, Some("client-shell")),
        "client-shell",
    );

    assert_eq!(
        buffer[merged].fg,
        super::super::status_color(AgentStatus::Blocked, &state.config.palette)
    );
}

#[test]
fn priority_order_does_not_gather_same_named_spaces() {
    let mut projected = split_project_snapshot();
    projected.agents[1].state_change_seq = 9;
    projected.agents[2].state_change_seq = 5;
    let mut config = Config::default();
    config.ui.agent_panel_sort = crate::config::AgentPanelSortConfig::Priority;

    let state = composed(&config, projected);

    assert_eq!(listed_agents(&state), ["pane_2", "pane_3", "pane_1"]);
}

pub(super) fn aged_agent(mut agent: ClientShellAgent, minutes_ago: u64) -> ClientShellAgent {
    // Half a minute into the bucket, so the label is the same wherever the clock ticks.
    let changed_at = crate::agent_state_time::unix_ms_now() - minutes_ago * 60_000 - 30_000;
    agent.tokens.push((
        crate::agent_state_time::STATE_CHANGED_AT_TOKEN.into(),
        changed_at.to_string(),
    ));
    agent
}

#[test]
fn agent_row_ends_with_how_long_ago_its_state_changed() {
    let mut projected = two_space_snapshot();
    projected.agents[0] = aged_agent(projected.agents[0].clone(), 3 * 60);
    let mut state = composed(&Config::default(), projected);
    let frame = state.compose(106, 30).expect("agent sidebar frame");
    let rows = frame_rows(&frame);

    let aged = agent_rect(&state, "pane_1");
    let first_line = rows[aged.y as usize]
        .chars()
        .skip(aged.x as usize)
        .take(aged.width as usize)
        .collect::<String>();
    assert!(first_line.ends_with(" 3h"), "row: {first_line:?}");
    assert!(state.hits.agent_ages_shown);

    // An agent whose server reports no change time keeps the whole row for its text.
    let plain = agent_rect(&state, "pane_2");
    let plain_line = rows[plain.y as usize]
        .chars()
        .skip(plain.x as usize)
        .take(plain.width as usize)
        .collect::<String>();
    assert!(!plain_line.trim_end().ends_with('h'), "row: {plain_line:?}");
}

#[test]
fn shown_ages_schedule_a_refresh_and_plain_rows_do_not() {
    let plain = two_space_state();
    assert!(!plain.hits.agent_ages_shown);
    assert_eq!(plain.agent_age_refresh_at, None);

    let mut projected = two_space_snapshot();
    projected.agents[0] = aged_agent(projected.agents[0].clone(), 5);
    let mut state = composed(&Config::default(), projected);
    let deadline = state.agent_age_refresh_at.expect("refresh is scheduled");

    assert!(!state.tick_agent_ages(deadline - std::time::Duration::from_secs(1)));
    assert!(state.tick_agent_ages(deadline));
    assert_eq!(state.agent_age_refresh_at, None);
}

#[test]
fn narrow_sidebar_drops_the_age_before_the_row_text() {
    let mut projected = two_space_snapshot();
    projected.agents[0] = aged_agent(projected.agents[0].clone(), 3 * 60);
    let mut config = Config::default();
    config.ui.sidebar_min_width = 10;
    config.ui.sidebar_width = 10;

    let state = composed(&config, projected);

    assert!(!state.hits.agents.is_empty());
    assert!(!state.hits.agent_ages_shown);
}

#[test]
fn mobile_switcher_offers_the_way_back_to_every_space() {
    let mobile_agents = |state: &ClientShellState| {
        state
            .hits
            .mobile_targets
            .iter()
            .filter(|(_, target)| matches!(target, ClientMobileTarget::Agent { .. }))
            .count()
    };
    let clear_rect = |state: &ClientShellState| {
        state
            .hits
            .mobile_targets
            .iter()
            .find(|(_, target)| *target == ClientMobileTarget::ClearAgentFilter)
            .map(|(rect, _)| *rect)
    };
    let mut state = two_space_state();
    state.mode = ClientShellMode::Navigate;
    state.compose(44, 30).expect("mobile switcher");
    assert_eq!(mobile_agents(&state), 2);
    assert_eq!(clear_rect(&state), None);

    state.config.agent_panel_filter = Some(super::super::agent_filter::AgentPanelFilter {
        endpoint_id: ClientEndpointId::Local,
        space: "second".into(),
    });
    state.compose(44, 30).expect("filtered mobile switcher");
    assert_eq!(mobile_agents(&state), 1);
    let clear = clear_rect(&state).expect("show all spaces action");

    state.handle_raw_events(vec![RawInputEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: clear.x,
        row: clear.y,
        modifiers: KeyModifiers::empty(),
    })]);
    state.handle_raw_events(vec![RawInputEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: clear.x,
        row: clear.y,
        modifiers: KeyModifiers::empty(),
    })]);
    state.compose(44, 30).expect("mobile switcher");

    assert_eq!(state.config.agent_panel_filter, None);
    assert_eq!(state.mode, ClientShellMode::Navigate);
    assert_eq!(mobile_agents(&state), 2);
}

#[test]
fn agent_icon_is_drawn_in_the_color_of_the_agent_state() {
    use crate::config::AgentSidebarToken;

    let mut projected = two_space_snapshot();
    projected.agents[0].agent = Some("claude".into());
    projected.agents[0].agent_status = AgentStatus::Blocked;
    let mut config = Config::default();
    config.ui.sidebar.agents.rows =
        vec![vec![AgentSidebarToken::AgentIcon, AgentSidebarToken::Agent]];
    let mut state = composed(&config, projected);
    let frame = state.compose(106, 30).expect("agent sidebar frame");
    let buffer = frame.to_ratatui_buffer().expect("buffer");

    let icon = cell_symbol_position(&frame, agent_rect(&state, "pane_1"), "✳");

    assert_eq!(
        buffer[icon].fg,
        super::super::status_color(AgentStatus::Blocked, &state.config.palette)
    );
}

fn fold_rect(state: &ClientShellState, space: &str) -> Rect {
    state
        .hits
        .space_collapse_toggles
        .iter()
        .find(|(_, name)| name == space)
        .map(|(rect, _)| *rect)
        .expect("fold control")
}

fn folded(space: &str) -> ClientShellState {
    let mut state = two_space_state();
    let fold = fold_rect(&state, space);
    click(&mut state, fold);
    state
}

#[test]
fn folding_a_space_keeps_its_header_and_hides_its_agents() {
    let mut state = two_space_state();
    assert_eq!(state.hits.space_collapse_toggles.len(), 2);

    let fold = fold_rect(&state, "second");
    let outcome = click(&mut state, fold);

    assert!(outcome.actions.is_empty() && outcome.requests.is_empty());
    assert_eq!(listed_agents(&state), ["pane_1"]);
    // Both headers stay, and the folded one says how many agents it holds.
    assert_eq!(state.hits.agent_group_headers.len(), 2);
    let frame = state.compose(106, 30).expect("agent sidebar frame");
    assert!(frame_rows(&frame).iter().any(|row| row.contains("1 ▸")));
    // The fold control did not select the space as its header would.
    assert_eq!(state.config.agent_panel_filter, None);

    let fold = fold_rect(&state, "second");
    click(&mut state, fold);
    assert_eq!(listed_agents(&state), ["pane_1", "pane_2"]);
}

#[test]
fn folded_agents_leave_keyboard_navigation() {
    let mut state = folded("client-shell");
    assert_eq!(listed_agents(&state), ["pane_2"]);

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
    // An index past the visible agents must not reach the server's unfolded order.
    let mut second = ClientShellInput::default();
    state.record_binding(
        crate::input::KeybindMatch::Action(crate::input::KeybindAction::FocusAgent(1)),
        &mut second,
    );
    assert!(second.actions.is_empty());
}

#[test]
fn folding_only_hides_agents_where_a_header_can_unfold_them() {
    // Priority order has no headers, so a fold left over from the grouped order hides nothing.
    let mut state = folded("second");
    state.config.agent_panel_sort = crate::config::AgentPanelSortConfig::Priority;
    state.compose(106, 30).expect("agent sidebar frame");
    assert_eq!(listed_agents(&state).len(), 2);
    assert!(state.hits.space_collapse_toggles.is_empty());

    // A selected space lists its agents even while it is folded in the grouped list.
    let mut state = folded("second");
    let chip = chip_rect(&state, Some("second"));
    click(&mut state, chip);
    assert_eq!(listed_agents(&state), ["pane_2"]);
}

#[test]
fn clicking_a_folded_header_still_selects_its_space() {
    let mut state = folded("second");
    let header = header_rect(&state, "second");

    click(&mut state, header);

    assert_eq!(listed_agents(&state), ["pane_2"]);
}

#[test]
fn keyboard_agent_navigation_scrolls_the_grouped_list_to_its_target() {
    let mut projected = two_space_snapshot();
    projected.agents = (1..=12)
        .map(|index| {
            let space = if index <= 6 { "ws_1" } else { "ws_2" };
            let tab = if index <= 6 { "tab_1" } else { "tab_2" };
            space_agent(&format!("pane_{index}"), space, tab, index == 1)
        })
        .collect();
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(projected));
    state.set_pane_surface(surface());
    state.compose(106, 20).expect("short agent sidebar");
    assert!(!listed_agents(&state).contains(&"pane_12"));

    let mut previous = ClientShellInput::default();
    state.record_binding(
        crate::input::KeybindMatch::Action(crate::input::KeybindAction::PreviousAgent),
        &mut previous,
    );
    state.compose(106, 20).expect("short agent sidebar");

    assert!(listed_agents(&state).contains(&"pane_12"));
}

#[test]
fn mobile_way_back_also_unfolds_spaces() {
    let mut state = folded("second");
    state.mode = ClientShellMode::Navigate;
    state.compose(44, 30).expect("mobile switcher");
    let clear = state
        .hits
        .mobile_targets
        .iter()
        .find(|(_, target)| *target == ClientMobileTarget::ClearAgentFilter)
        .map(|(rect, _)| *rect)
        .expect("show all spaces action");

    for kind in [
        MouseEventKind::Down(MouseButton::Left),
        MouseEventKind::Up(MouseButton::Left),
    ] {
        state.handle_raw_events(vec![RawInputEvent::Mouse(MouseEvent {
            kind,
            column: clear.x,
            row: clear.y,
            modifiers: KeyModifiers::empty(),
        })]);
    }

    assert!(state.config.collapsed_spaces.is_empty());
}
