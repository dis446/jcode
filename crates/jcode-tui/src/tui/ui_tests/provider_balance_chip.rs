//! Where the provider credit chip lands in the real frame, checked on the
//! rendered buffer rather than by inspecting the draw helper in isolation.

use super::*;
use ratatui::{Terminal, backend::TestBackend};

fn chip() -> crate::tui::provider_balance::Chip {
    crate::tui::provider_balance::Chip {
        label: "DeepSeek $27.89".into(),
        fallbacks: Vec::new(),
        low: false,
        exhausted: false,
    }
}

fn render(state: &TestState, width: u16, height: u16) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| crate::tui::ui::draw(frame, state))
        .unwrap();
    let buffer = terminal.backend().buffer();
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect()
}

fn chip_position(rows: &[String]) -> Option<(usize, usize)> {
    rows.iter()
        .enumerate()
        .find_map(|(y, row)| row.find("DeepSeek $27.89").map(|column| (y, column)))
}

#[test]
fn chip_sits_at_the_right_edge_of_the_status_line() {
    let _env = crate::storage::lock_test_env();
    let _render = viewport_snapshot_test_lock();
    let state = TestState {
        provider_balance_chip: Some(chip()),
        ..Default::default()
    };
    let rows = render(&state, 100, 30);
    let (row, column) = chip_position(&rows).expect("chip renders");
    assert_eq!(
        column,
        100 - "DeepSeek $27.89".len() - 1,
        "one cell of right margin: {:?}",
        rows[row]
    );
    assert!(row > 0, "chip is not on the header row");
}

#[test]
fn chip_is_dropped_instead_of_overlapping_or_wrapping() {
    let _env = crate::storage::lock_test_env();
    let _render = viewport_snapshot_test_lock();
    let state = TestState {
        provider_balance_chip: Some(chip()),
        ..Default::default()
    };
    // Too narrow for the label plus a right margin.
    let rows = render(&state, 14, 16);
    assert!(chip_position(&rows).is_none(), "{rows:?}");
}

#[test]
fn compact_quota_label_is_used_when_the_full_label_does_not_fit() {
    let _env = crate::storage::lock_test_env();
    let _render = viewport_snapshot_test_lock();
    let state = TestState {
        provider_balance_chip: Some(crate::tui::provider_balance::Chip {
            label: "OpenCode Go 5h 2% (1h 14m) · 1w 20% (3d 8h) · 1mo 31% (15d 19h)".into(),
            fallbacks: vec!["OpenCode Go 31%".into()],
            low: false,
            exhausted: false,
        }),
        ..Default::default()
    };
    let rows = render(&state, 40, 20);
    assert!(
        rows.iter().any(|row| row.contains("OpenCode Go 31%")),
        "{rows:?}"
    );
    assert!(!rows.iter().any(|row| row.contains("(1h 14m)")), "{rows:?}");
}

#[test]
fn no_chip_means_no_extra_chrome() {
    let _env = crate::storage::lock_test_env();
    let _render = viewport_snapshot_test_lock();
    let rows = render(&TestState::default(), 100, 30);
    assert!(chip_position(&rows).is_none(), "{rows:?}");
}
