//! Client-local space filter for the Agents panel.
//!
//! Choosing a space is presentation state: it never reaches the server, and it narrows whatever
//! the server-owned agent view already selected instead of replacing it.

use super::render::{display_width, put_text};
use super::*;

const ALL_LABEL: &str = "all";
const CHIP_LABEL_MAX_WIDTH: usize = 16;
/// Divider and title rows above the chip strip.
const PANEL_TITLE_ROWS: u16 = 2;
/// Rows the Agents list starts below the panel top when no chips are drawn.
pub(super) const AGENT_LIST_OFFSET: u16 = 3;

/// Spaces in Spaces-panel order, each with the name that panel lists it under.
pub(super) type SpaceLabels<'a> = Vec<(&'a ClientShellWorkspace, &'a str)>;

/// The space the Agents panel is narrowed to: every workspace on one machine that is listed
/// under `space`. Workspaces opened on the same directory share a name, and the user thinks of
/// them as one project, so the name is the identity rather than a single workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AgentPanelFilter {
    pub(super) endpoint_id: ClientEndpointId,
    pub(super) space: String,
}

impl AgentPanelFilter {
    /// The space name this filter keeps among one machine's `spaces`.
    ///
    /// A filter naming another machine or a space that has since closed is inert, so a stale
    /// choice shows every agent instead of an empty panel nobody can explain.
    pub(super) fn space_among<'a>(
        &'a self,
        endpoint_id: &ClientEndpointId,
        spaces: &[(&ClientShellWorkspace, &str)],
    ) -> Option<&'a str> {
        (&self.endpoint_id == endpoint_id && spaces.iter().any(|(_, name)| *name == self.space))
            .then_some(self.space.as_str())
    }
}

impl ClientShellConfig {
    /// Space name the Agents panel is narrowed to inside one machine's snapshot.
    pub(super) fn agent_filter_space(
        &self,
        endpoint_id: &ClientEndpointId,
        snapshot: &ClientShellSnapshot,
    ) -> Option<&str> {
        let filter = self.agent_panel_filter.as_ref()?;
        if &filter.endpoint_id != endpoint_id {
            return None;
        }
        filter.space_among(endpoint_id, &space_labels(snapshot))
    }
}

/// The filter, when it still names a space on a machine with a cached snapshot.
pub(super) fn live_filter<'a>(
    filter: Option<&'a AgentPanelFilter>,
    endpoints: &[ClientShellEndpoint],
) -> Option<&'a AgentPanelFilter> {
    filter.filter(|filter| {
        endpoints
            .iter()
            .filter(|endpoint| endpoint.endpoint_id == filter.endpoint_id)
            .filter_map(|endpoint| endpoint.snapshot.as_deref())
            .any(|snapshot| {
                filter
                    .space_among(&filter.endpoint_id, &space_labels(snapshot))
                    .is_some()
            })
    })
}

pub(super) struct AgentFilterChip<'a> {
    endpoint_id: &'a ClientEndpointId,
    space: &'a str,
    /// Most urgent rolled-up agent state among the workspaces sharing this name.
    status: crate::api::schema::AgentStatus,
    populated: bool,
}

pub(super) fn space_labels(snapshot: &ClientShellSnapshot) -> SpaceLabels<'_> {
    super::render::workspace_entries(snapshot, &HashSet::new())
        .into_iter()
        .filter_map(|entry| {
            let workspace = snapshot.workspaces.get(entry.index)?;
            Some((
                workspace,
                super::sidebar::workspace_display_label(workspace, entry.indented),
            ))
        })
        .collect()
}

/// Name the workspace is listed under, falling back to its id for a workspace `spaces` lacks.
pub(super) fn space_name<'a>(
    spaces: &[(&ClientShellWorkspace, &'a str)],
    workspace_id: &'a str,
) -> &'a str {
    spaces
        .iter()
        .find(|(workspace, _)| workspace.workspace_id == workspace_id)
        .map_or(workspace_id, |(_, name)| name)
}

/// One chip per space name that has agents, in Spaces-panel order.
///
/// The selected space keeps its chip after its last agent leaves, so the filter that empties the
/// list stays visible and can be clicked off.
pub(super) fn snapshot_chips<'a>(
    endpoint_id: &'a ClientEndpointId,
    snapshot: &'a ClientShellSnapshot,
    spaces: &[(&'a ClientShellWorkspace, &'a str)],
    active: Option<&AgentPanelFilter>,
) -> Vec<AgentFilterChip<'a>> {
    let selected = active.and_then(|filter| filter.space_among(endpoint_id, spaces));
    if selected.is_none() && snapshot.agents.is_empty() {
        return Vec::new();
    }
    let populated = snapshot
        .agents
        .iter()
        .map(|agent| agent.workspace_id.as_str())
        .collect::<HashSet<_>>();
    let mut chips = Vec::<AgentFilterChip<'a>>::new();
    for (workspace, space) in spaces {
        let populated = populated.contains(workspace.workspace_id.as_str());
        match chips.iter_mut().find(|chip| chip.space == *space) {
            Some(chip) => {
                chip.populated |= populated;
                if status_priority(workspace.agent_status) > status_priority(chip.status) {
                    chip.status = workspace.agent_status;
                }
            }
            None => chips.push(AgentFilterChip {
                endpoint_id,
                space,
                status: workspace.agent_status,
                populated,
            }),
        }
    }
    chips.retain(|chip| chip.populated || selected == Some(chip.space));
    chips
}

pub(super) fn endpoint_chips<'a>(
    endpoints: &'a [ClientShellEndpoint],
    active: Option<&AgentPanelFilter>,
) -> Vec<AgentFilterChip<'a>> {
    endpoints
        .iter()
        .filter_map(|endpoint| {
            endpoint.snapshot.as_deref().map(|snapshot| {
                snapshot_chips(
                    &endpoint.endpoint_id,
                    snapshot,
                    &space_labels(snapshot),
                    active,
                )
            })
        })
        .flatten()
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ChipSlot {
    /// Position in the chip list; `0` is the leading `all` chip.
    index: usize,
    row: u16,
    x: u16,
    width: u16,
}

#[derive(Debug, PartialEq, Eq)]
struct ChipLayout {
    slots: Vec<ChipSlot>,
    hidden: usize,
}

fn overflow_marker(hidden: usize) -> String {
    format!(" +{hidden}")
}

fn place_chips(
    order: &[usize],
    widths: &[u16],
    row_width: u16,
    max_rows: u16,
    active: usize,
) -> ChipLayout {
    let mut slots = Vec::with_capacity(order.len());
    let mut hidden = 0;
    let (mut row, mut x) = (0u16, 0u16);
    for (position, &index) in order.iter().enumerate() {
        let width = widths[index].min(row_width);
        if x > 0 && x.saturating_add(width) > row_width {
            row += 1;
            x = 0;
        }
        if row >= max_rows {
            hidden = order.len() - position;
            break;
        }
        slots.push(ChipSlot {
            index,
            row,
            x,
            width,
        });
        x += width;
    }
    // Trade trailing chips for the overflow marker, but never the two the user must keep:
    // the way back to every space and the space currently selected.
    while hidden > 0 {
        let Some(last) = slots.last().copied() else {
            break;
        };
        let marker = display_width(&overflow_marker(hidden));
        if last.x + last.width + marker <= row_width || last.index == 0 || last.index == active {
            break;
        }
        slots.pop();
        hidden += 1;
    }
    ChipLayout { slots, hidden }
}

/// Wraps chips into at most `max_rows` rows of `row_width` cells.
///
/// When the strip overflows, the selected chip moves next to `all` so the active filter stays
/// visible and can be clicked off.
fn layout_chips(widths: &[u16], row_width: u16, max_rows: u16, active: usize) -> ChipLayout {
    if widths.is_empty() || row_width == 0 || max_rows == 0 {
        return ChipLayout {
            slots: Vec::new(),
            hidden: widths.len(),
        };
    }
    let natural = (0..widths.len()).collect::<Vec<_>>();
    let layout = place_chips(&natural, widths, row_width, max_rows, active);
    if active == 0 || layout.slots.iter().any(|slot| slot.index == active) {
        return layout;
    }
    let promoted = [0, active]
        .into_iter()
        .chain(
            natural
                .into_iter()
                .filter(|index| *index != 0 && *index != active),
        )
        .collect::<Vec<_>>();
    place_chips(&promoted, widths, row_width, max_rows, active)
}

/// Draws the space chips under the Agents title and returns the row where the list starts,
/// relative to the top of `area`.
///
/// `chips` is only called once the strip can be drawn, so a frame without it pays nothing.
pub(super) fn render_chips<'a>(
    buffer: &mut Buffer,
    area: Rect,
    chips: impl FnOnce() -> Vec<AgentFilterChip<'a>>,
    active: Option<&AgentPanelFilter>,
    config: &ClientShellConfig,
    hits: &mut ShellHitMap,
) -> u16 {
    // An active filter always shows its chip so it can be seen and clicked off. Otherwise the
    // strip is no use without a mouse.
    if active.is_none() && !config.mouse_capture {
        return AGENT_LIST_OFFSET;
    }
    // The list keeps at least one row below the title rows, one chip row, and the spacer.
    let Some(list_rows) = area
        .height
        .checked_sub(AGENT_LIST_OFFSET)
        .filter(|rows| *rows >= 2)
    else {
        return AGENT_LIST_OFFSET;
    };
    let chips = chips();
    // One populated space leaves nothing to choose between.
    if active.is_none() && chips.len() < 2 {
        return AGENT_LIST_OFFSET;
    }
    let row_width = area.width.saturating_sub(1);
    let labels = std::iter::once(ALL_LABEL)
        .chain(chips.iter().map(|chip| chip.space))
        .map(|label| format!(" {} ", crate::ui::truncate_end(label, CHIP_LABEL_MAX_WIDTH)))
        .collect::<Vec<_>>();
    let widths = labels
        .iter()
        .map(|label| display_width(label))
        .collect::<Vec<_>>();
    let active_index = active
        .and_then(|filter| {
            chips.iter().position(|chip| {
                chip.endpoint_id == &filter.endpoint_id && chip.space == filter.space
            })
        })
        .map_or(0, |position| position + 1);
    // Chips may take a third of the rows the list would otherwise have.
    let max_rows = (list_rows / 3).max(1);
    let layout = layout_chips(&widths, row_width, max_rows, active_index);
    let Some(last) = layout.slots.last().copied() else {
        return AGENT_LIST_OFFSET;
    };

    let palette = &config.palette;
    let origin_x = area.x.saturating_add(1);
    let origin_y = area.y.saturating_add(PANEL_TITLE_ROWS);
    for slot in &layout.slots {
        let rect = Rect::new(origin_x + slot.x, origin_y + slot.row, slot.width, 1);
        let chip = slot.index.checked_sub(1).and_then(|index| chips.get(index));
        let style = if slot.index == active_index {
            Style::default()
                .fg(palette.accent)
                .bg(palette.active_row_bg)
                .add_modifier(Modifier::BOLD)
        } else {
            // A space waiting on the user stays noticeable while another space is selected.
            let attention = chip.map(|chip| chip.status).filter(|status| {
                matches!(
                    status,
                    crate::api::schema::AgentStatus::Blocked
                        | crate::api::schema::AgentStatus::Done
                )
            });
            Style::default()
                .fg(attention.map_or(palette.overlay0, |status| status_color(status, palette)))
        };
        put_text(
            buffer,
            rect.x,
            rect.y,
            rect.width,
            &labels[slot.index],
            style,
        );
        if config.mouse_capture {
            let target = chip.map(|chip| AgentPanelFilter {
                endpoint_id: chip.endpoint_id.clone(),
                space: chip.space.to_owned(),
            });
            hits.agent_filter_chips.push((rect, target));
        }
    }
    if layout.hidden > 0 {
        let x = last.x + last.width;
        put_text(
            buffer,
            origin_x + x,
            origin_y + last.row,
            row_width.saturating_sub(x),
            &overflow_marker(layout.hidden),
            Style::default()
                .fg(palette.overlay0)
                .add_modifier(Modifier::DIM),
        );
    }
    PANEL_TITLE_ROWS + last.row + 2
}

#[cfg(test)]
mod tests {
    use super::*;

    fn indices(layout: &ChipLayout) -> Vec<(usize, u16)> {
        layout
            .slots
            .iter()
            .map(|slot| (slot.index, slot.row))
            .collect()
    }

    #[test]
    fn chips_wrap_when_a_row_is_full() {
        let layout = layout_chips(&[5, 10, 10, 7], 20, 3, 0);

        assert_eq!(indices(&layout), [(0, 0), (1, 0), (2, 1), (3, 1)]);
        assert_eq!(layout.hidden, 0);
        assert_eq!(layout.slots[2].x, 0);
        assert_eq!(layout.slots[3].x, 10);
    }

    #[test]
    fn overflow_keeps_room_for_the_marker() {
        // Four chips of 10 fit two per row; one row leaves no cell for " +2".
        let layout = layout_chips(&[10, 10, 10, 10], 20, 1, 0);

        assert_eq!(indices(&layout), [(0, 0)]);
        assert_eq!(layout.hidden, 3);
    }

    #[test]
    fn overflow_promotes_the_selected_chip_next_to_all() {
        let layout = layout_chips(&[5, 8, 8, 8], 20, 1, 3);

        assert_eq!(indices(&layout), [(0, 0), (3, 0)]);
        assert_eq!(layout.hidden, 2);
    }

    #[test]
    fn a_chip_wider_than_the_row_is_clipped_not_dropped() {
        let layout = layout_chips(&[5, 30], 12, 2, 0);

        assert_eq!(indices(&layout), [(0, 0), (1, 1)]);
        assert_eq!(layout.slots[1].width, 12);
    }

    #[test]
    fn no_room_hides_every_chip() {
        assert_eq!(
            layout_chips(&[5, 8], 0, 2, 0),
            ChipLayout {
                slots: Vec::new(),
                hidden: 2
            }
        );
    }
}
