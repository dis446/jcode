"""`@file` picker suite (fork ledger C1). Meant for the fork build only.

The stock binary has no popup, so this suite is expected to FAIL there: that
failure is the proof the tests detect the feature.
"""
from harness import Session

SUITE = "at_file"
FILES = ["README.md", "src/main.rs", "src/lib.rs", "my dir/note.txt", ".env", "docs/readme.txt"]


def composer(screen):
    return screen.split("1>")[-1]


def t_at_opens_popup_best_match_first(b, v):
    with Session(b, files=FILES) as s:
        s.wait_ready()
        s.type("@readm")
        screen = s.wait_text("▸ README.md")
        assert screen.index("▸ README.md") < screen.index("readme.txt") if "readme.txt" in screen else True


def t_scoped_dir_lists_children(b, v):
    with Session(b, files=FILES) as s:
        s.wait_ready()
        s.type("@src/")
        screen = s.wait_text("src/main.rs")
        assert "src/lib.rs" in screen


def t_enter_accepts_file_adds_space_no_submit(b, v):
    with Session(b, files=FILES) as s:
        s.wait_ready()
        s.type("see @readm")
        s.wait_text("▸ README.md")
        s.key("Enter")
        s.wait_text("1> see @README.md")
        screen = s.settle()
        assert "▸" not in screen, "popup should close after accept\n" + screen
        assert "2>" not in screen, "Enter must not submit\n" + screen


def t_tab_on_dir_keeps_drilling(b, v):
    with Session(b, files=FILES) as s:
        s.wait_ready()
        s.type("@sr")
        s.wait_text("▸ src/")
        s.key("Tab")
        s.wait_text("1> @src/")
        s.wait_text("src/main.rs")


def t_esc_closes_popup_keeps_input(b, v):
    with Session(b, files=FILES) as s:
        s.wait_ready()
        s.type("@rea")
        s.wait_text("▸")
        s.key("Escape")
        s.wait_gone("▸")
        assert "1> @rea" in s.settle()


def t_down_moves_selection(b, v):
    with Session(b, files=["a1.rs", "a2.rs"]) as s:
        s.wait_ready()
        s.type("@a")
        s.wait_text("▸ a1.rs")
        s.key("Down")
        s.wait_text("▸ a2.rs")


def t_quoted_flow_with_spaces(b, v):
    with Session(b, files=FILES) as s:
        s.wait_ready()
        s.type('@"my')
        s.wait_text("my dir/")
        s.key("Tab")
        s.wait_text('1> @"my dir/')


def t_wrapper_prefix_opens_popup(b, v):
    with Session(b, files=FILES) as s:
        s.wait_ready()
        s.type("(@readm")
        s.wait_text("▸ README.md")


def t_hidden_files_included(b, v):
    with Session(b, files=FILES) as s:
        s.wait_ready()
        s.type("@.en")
        s.wait_text("▸ .env")


def t_tab_forces_completion_on_plain_token(b, v):
    with Session(b, files=["only_one_file.txt"]) as s:
        s.wait_ready()
        s.type("cat only_one")
        s.key("Tab")
        s.wait_text("1> cat only_one_file.txt")


def t_slash_unaffected_while_files_match(b, v):
    with Session(b, files=["model.rs"]) as s:
        s.wait_ready()
        s.type("/mod")
        screen = s.wait_text("/model  List or switch models")
        assert "▸" not in screen


def t_popup_never_wraps_on_narrow_terminal(b, v):
    with Session(b, files=["a_really_long_file_name_that_overflows_everything.rs"], cols=40, rows=24) as s:
        s.wait_ready()
        s.type("@a_really")
        screen = s.wait_text("▸")
        for line in screen.splitlines():
            assert len(line) <= 40, f"line wider than terminal: {line!r}"


def t_backspace_updates_then_closes_popup(b, v):
    with Session(b, files=FILES) as s:
        s.wait_ready()
        s.type("@readm")
        s.wait_text("▸ README.md")
        s.key("BSpace")
        s.wait_text("1> @read")
        s.wait_text("▸ README.md")
        for _ in range(5):
            s.key("BSpace")
        # Empty composer on a fresh session shows stock onboarding (no `1>`), so only
        # assert the popup is gone.
        s.wait_gone("▸ README.md")
