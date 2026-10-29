"""Project-defined slash commands (fork ledger C5). Fork build only.

The stock binary has no `.jcode/commands.json` support, so these tests fail
there by design.
"""
import json
import os

from harness import Session

SUITE = "project_commands"


def install_commands(session, commands):
    config = os.path.join(session.work, ".jcode")
    os.makedirs(config, exist_ok=True)
    with open(os.path.join(config, "commands.json"), "w") as handle:
        json.dump({"commands": commands}, handle)


def scroll_help_to(session, needle):
    """Page through the /help overlay until `needle` is rendered."""
    for _ in range(40):
        screen = session.screen()
        if needle in screen:
            return screen
        session.key("PageDown")
        session.settle(quiet=0.1)
    raise AssertionError(f"{needle!r} not found in help overlay")


def t_project_command_runs_shell_without_a_turn(b, v):
    with Session(b) as s:
        install_commands(s, [
            {
                "name": "e2e-say",
                "description": "Say a fixed line",
                "command": "printf e2e-project-ok",
            }
        ])
        s.wait_ready()
        s.type("/e2e-say")
        s.key("Enter")
        screen = s.wait_text("e2e-project-ok")
        assert "e2e-say" in screen


def t_project_command_arguments_reach_the_shell_verbatim(b, v):
    with Session(b) as s:
        install_commands(s, [
            {
                "name": "e2e-echo",
                "description": "Echo the typed arguments",
                "command": "printf '<%s>'",
            }
        ])
        s.wait_ready()
        s.type('/e2e-echo "a b" back-end/state-machine')
        s.key("Enter")
        screen = s.wait_text("<back-end/state-machine>")
        assert "<a b>" in screen, screen


def t_project_command_appears_in_completion_and_help(b, v):
    with Session(b) as s:
        install_commands(s, [
            {
                "name": "e2e-listed",
                "description": "Listed in help",
                "command": "true",
            }
        ])
        s.wait_ready()
        s.type("/e2e-")
        s.wait_text("/e2e-listed")
        s.key("Escape")
        s.key("C-u")
        s.type("/help")
        s.key("Enter")
        scroll_help_to(s, "Project commands")
        s.wait_text("Listed in help")


def t_project_command_confirm_requires_yes(b, v):
    with Session(b) as s:
        install_commands(s, [
            {
                "name": "e2e-danger",
                "description": "A gated command",
                "command": "printf e2e-danger-ran",
                "confirm": True,
            }
        ])
        s.wait_ready()
        s.type("/e2e-danger")
        s.key("Enter")
        screen = s.wait_text("Press y to run")
        assert "e2e-danger-ran" not in screen, screen
        s.type("n")
        s.key("Enter")
        s.wait_text("Cancelled /e2e-danger")
        assert "e2e-danger-ran" not in s.settle(), "n must not run the command"


def t_built_in_command_wins_over_a_project_command(b, v):
    with Session(b) as s:
        install_commands(s, [
            {
                "name": "help",
                "description": "Should never shadow /help",
                "command": "printf e2e-shadow-ran",
            }
        ])
        s.wait_ready()
        s.type("/help")
        s.key("Enter")
        s.wait_text("Show this help overlay")
        assert "e2e-shadow-ran" not in s.settle()


def t_initial_message_prefills_without_sending(b, v):
    with Session(b, args=["--initial-message", "draft-only-123"]) as s:
        s.wait_ready()
        screen = s.wait_text("1> draft-only-123")
        settled = s.settle()
        assert "1> draft-only-123" in settled, settled
        assert "2>" not in settled, "the draft must not start a turn\n" + screen
