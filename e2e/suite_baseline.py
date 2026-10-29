"""Baseline regression suite. Must pass on stock upstream AND on the fork.

Pins behavior the fork must never change.
"""
from harness import Session

SUITE = "baseline"


def t_starts_and_shows_composer(b, expect_version):
    with Session(b) as s:
        screen = s.wait_ready()
        assert "1>" in screen, screen
        assert s.alive()


def t_version_command_reports_build(b, expect_version):
    if not expect_version:
        return
    with Session(b) as s:
        s.wait_ready()
        s.type("/version")
        s.key("Enter")
        s.wait_text(f"v{expect_version}")


def t_typing_echoes_and_ctrl_u_clears(b, v):
    with Session(b) as s:
        s.wait_ready()
        s.type("hello world")
        s.wait_text("1> hello world")
        s.key("C-u")
        s.wait_gone("hello world")


def t_slash_palette_lists_model(b, v):
    with Session(b) as s:
        s.wait_ready()
        s.type("/mod")
        screen = s.wait_text("/model  List or switch models")
        assert "/models" in screen


def t_slash_tab_completes_command(b, v):
    with Session(b) as s:
        s.wait_ready()
        s.type("/hel")
        s.wait_text("/help")
        s.key("Tab")
        s.wait_for(lambda sc: "1> /help" in sc, "tab completes /hel to /help")


def t_slash_palette_closes_on_clear(b, v):
    with Session(b) as s:
        s.wait_ready()
        s.type("/mod")
        s.wait_text("List or switch models")
        s.key("C-u")
        s.wait_gone("List or switch models")


def t_ctrl_z_restores_cleared_input(b, v):
    with Session(b) as s:
        s.wait_ready()
        s.type("keep me")
        s.wait_text("keep me")
        s.key("C-u")
        s.wait_gone("keep me")
        s.key("C-z")
        s.wait_text("1> keep me")


def t_email_like_at_sign_is_plain_text(b, v):
    """`x@y` is not a mention token: no popup, text stays literal."""
    with Session(b, files=["README.md"]) as s:
        s.wait_ready()
        s.type("mail me at x@rea")
        screen = s.wait_text("1> mail me at x@rea")
        assert "README.md" not in screen.split("1>")[-1], screen


def t_bracketed_paste_with_at_is_literal(b, v):
    with Session(b, files=["README.md"]) as s:
        s.wait_ready()
        s.paste("@readm")
        screen = s.wait_text("@readm")
        assert "README.md" not in screen.split("1>")[-1], screen


def t_long_input_does_not_crash(b, v):
    with Session(b) as s:
        s.wait_ready()
        s.type("x" * 600)
        s.settle()
        assert s.alive()
        s.key("C-u")
        s.wait_gone("xxxxxxxxxx")


def t_resize_survives(b, v):
    with Session(b) as s:
        s.wait_ready()
        s.type("still here")
        s._tmux("resize-window", "-t", s.name, "-x", "60", "-y", "18")
        s.wait_text("still here")
        s._tmux("resize-window", "-t", s.name, "-x", "140", "-y", "40")
        s.wait_text("still here")
        assert s.alive()
