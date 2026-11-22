use super::agent_filter::{
    agent_rect, chip_rect, click, listed_agents, space_agent, split_project_snapshot,
    two_space_snapshot,
};
use super::*;

fn tree_config() -> Config {
    let mut config = Config::default();
    config.ui.sidebar.layout = crate::config::SidebarLayoutConfig::Tree;
    config
}

fn tree_state(config: &Config, projected: ClientShellSnapshot) -> ClientShellState {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(config));
    state.set_snapshot(Box::new(projected));
    state.set_pane_surface(surface());
    state.compose(106, 30).expect("tree sidebar frame");
    state
}

fn space_rows(state: &ClientShellState) -> Vec<&str> {
    state
        .hits
        .workspaces
        .iter()
        .map(|hit| hit.workspace_id.as_str())
        .collect()
}

fn toggle_rect(state: &ClientShellState, space: &str) -> Rect {
    state
        .hits
        .space_collapse_toggles
        .iter()
        .find(|(_, name)| name == space)
        .map(|(rect, _)| *rect)
        .expect("collapse toggle")
}

fn focused_pane(outcome: &ClientShellInput) -> Option<&str> {
    match &outcome.actions[..] {
        [ClientShellAction::Endpoint { request, .. }] => match &request.method {
            crate::api::schema::Method::PaneFocus(target) => Some(target.pane_id.as_str()),
            _ => None,
        },
        _ => None,
    }
}

#[test]
fn tree_lists_each_space_once_with_its_agents_beneath() {
    let state = tree_state(&tree_config(), split_project_snapshot());

    // `client-shell` is two workspaces; the tree shows one row for them and one for `second`.
    assert_eq!(space_rows(&state), ["ws_1", "ws_2"]);
    assert_eq!(listed_agents(&state), ["pane_1", "pane_3", "pane_2"]);
    let first_space = state.hits.workspaces[0].rect;
    let second_space = state.hits.workspaces[1].rect;
    assert_eq!(agent_rect(&state, "pane_1").y, first_space.bottom());
    assert!(second_space.y > agent_rect(&state, "pane_3").bottom());
    assert_eq!(agent_rect(&state, "pane_2").y, second_space.bottom());
    // One list replaces both panels, so the split layout's regions stay empty.
    assert!(state.hits.workspace_body.is_empty());
    assert!(state.hits.agent_group_headers.is_empty());
    assert!(state.hits.sidebar_section_divider.is_empty());
}

#[test]
fn tree_lists_a_space_without_agents_as_a_bare_row() {
    let mut projected = two_space_snapshot();
    projected.agents.truncate(1);

    let state = tree_state(&tree_config(), projected);

    assert_eq!(space_rows(&state), ["ws_1", "ws_2"]);
    assert_eq!(listed_agents(&state), ["pane_1"]);
    assert_eq!(state.hits.space_collapse_toggles.len(), 1);
}

#[test]
fn tree_puts_the_agent_that_needs_the_user_first_in_its_space() {
    let mut projected = split_project_snapshot();
    projected.agents[2].agent_status = AgentStatus::Blocked;
    let mut config = tree_config();
    // The tree is grouped by nature; the split layout's order toggle does not apply to it.
    config.ui.agent_panel_sort = crate::config::AgentPanelSortConfig::Priority;

    let mut state = tree_state(&config, projected);

    assert_eq!(listed_agents(&state), ["pane_3", "pane_1", "pane_2"]);
    let mut first = ClientShellInput::default();
    state.record_binding(
        crate::input::KeybindMatch::Action(crate::input::KeybindAction::FocusAgent(0)),
        &mut first,
    );
    assert_eq!(focused_pane(&first), Some("pane_3"));
}

#[test]
fn collapsing_a_space_hides_its_agents_from_the_list_and_from_navigation() {
    let path = std::env::temp_dir().join(format!(
        "herdr-shell-collapsed-spaces-{}-{}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos()
    ));
    let config = ClientShellConfig::from_config(&tree_config()).with_preferences_path(path.clone());
    let mut state = ClientShellState::new(config);
    state.set_snapshot(Box::new(split_project_snapshot()));
    state.set_pane_surface(surface());
    state.compose(106, 30).expect("tree sidebar frame");

    let toggle = toggle_rect(&state, "client-shell");
    let outcome = click(&mut state, toggle);

    assert!(outcome.actions.is_empty() && outcome.requests.is_empty());
    assert_eq!(listed_agents(&state), ["pane_2"]);
    let frame = state.compose(106, 30).expect("tree sidebar frame");
    assert!(frame_rows(&frame).iter().any(|row| row.contains("2 ▸")));
    let mut first = ClientShellInput::default();
    state.record_binding(
        crate::input::KeybindMatch::Action(crate::input::KeybindAction::FocusAgent(0)),
        &mut first,
    );
    assert_eq!(focused_pane(&first), Some("pane_2"));
    // An index past the visible agents must not reach the server's uncollapsed order.
    let mut second = ClientShellInput::default();
    state.record_binding(
        crate::input::KeybindMatch::Action(crate::input::KeybindAction::FocusAgent(1)),
        &mut second,
    );
    assert!(second.actions.is_empty());

    let reloaded = ClientShellState::new(
        ClientShellConfig::from_config(&tree_config()).with_preferences_path(path.clone()),
    );
    assert!(reloaded.config.collapsed_spaces.contains("client-shell"));

    let toggle = toggle_rect(&state, "client-shell");
    click(&mut state, toggle);
    assert_eq!(listed_agents(&state), ["pane_1", "pane_3", "pane_2"]);
    std::fs::remove_file(path).expect("remove collapsed space preferences");
}

#[test]
fn clicking_a_space_row_focuses_its_workspace() {
    let mut state = tree_state(&tree_config(), split_project_snapshot());
    let row = state.hits.workspaces[1].rect;

    state.handle_raw_events(vec![RawInputEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: row.x + 2,
        row: row.y,
        modifiers: KeyModifiers::empty(),
    })]);
    let release = state.handle_raw_events(vec![RawInputEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: row.x + 2,
        row: row.y,
        modifiers: KeyModifiers::empty(),
    })]);

    assert!(matches!(
        &release.actions[..],
        [ClientShellAction::Endpoint { request, .. }]
            if matches!(
                &request.method,
                crate::api::schema::Method::WorkspaceFocus(target) if target.workspace_id == "ws_2"
            )
    ));
}

#[test]
fn space_chip_narrows_the_tree_to_that_space() {
    let mut state = tree_state(&tree_config(), split_project_snapshot());

    let chip = chip_rect(&state, Some("second"));
    click(&mut state, chip);

    assert_eq!(space_rows(&state), ["ws_2"]);
    assert_eq!(listed_agents(&state), ["pane_2"]);
}

#[test]
fn keyboard_agent_navigation_scrolls_the_tree_to_its_target() {
    let mut projected = snapshot();
    projected.agents = (1..=12)
        .map(|index| space_agent(&format!("pane_{index}"), "ws_1", "tab_1", index == 1))
        .collect();
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&tree_config()));
    state.set_snapshot(Box::new(projected));
    state.set_pane_surface(surface());
    state.compose(106, 14).expect("short tree sidebar");
    assert!(!listed_agents(&state).contains(&"pane_12"));

    let mut previous = ClientShellInput::default();
    state.record_binding(
        crate::input::KeybindMatch::Action(crate::input::KeybindAction::PreviousAgent),
        &mut previous,
    );
    assert_eq!(focused_pane(&previous), Some("pane_12"));
    state.compose(106, 14).expect("short tree sidebar");

    assert!(listed_agents(&state).contains(&"pane_12"));
    assert_eq!(state.reveal_tree_agent, None);
}

#[test]
fn split_layout_is_untouched_by_the_tree_option() {
    let state = tree_state(&Config::default(), split_project_snapshot());

    assert_eq!(space_rows(&state), ["ws_1", "ws_2", "ws_3"]);
    assert!(!state.hits.workspace_body.is_empty());
    assert!(state.hits.space_collapse_toggles.is_empty());
}
