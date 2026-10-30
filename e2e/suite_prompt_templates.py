"""Markdown prompt templates (fork ledger C9). Fork build only.

The stock binary has no prompt-template support, so these tests fail by design
there. A template expands client-side into a normal user message, so the tests
assert on the transcript instead of on a model response.
"""
import json
import os

from harness import Session

SUITE = "prompt_templates"


def install_template(session, name, content):
    prompts = os.path.join(session.work, ".jcode", "prompts")
    os.makedirs(prompts, exist_ok=True)
    with open(os.path.join(prompts, f"{name}.md"), "w") as handle:
        handle.write(content)


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


def t_template_expands_with_arguments(b, v):
    with Session(b) as s:
        install_template(s, "e2e-review", "TEMPLATE-MARK focus=${1:-all}\n")
        s.wait_ready()
        s.type("/e2e-review auth")
        s.key("Enter")
        s.wait_text("TEMPLATE-MARK focus=auth")


def t_template_without_arguments_uses_default(b, v):
    with Session(b) as s:
        install_template(s, "e2e-review", "TEMPLATE-DEFAULT focus=${1:-all}\n")
        s.wait_ready()
        s.type("/e2e-review")
        s.key("Enter")
        s.wait_text("TEMPLATE-DEFAULT focus=all")


def t_template_appears_in_completion_and_help(b, v):
    with Session(b) as s:
        install_template(
            s,
            "e2e-listed",
            "---\ndescription: Listed template\n---\nTEMPLATE-LISTED body\n",
        )
        s.wait_ready()
        s.type("/e2e-li")
        s.wait_text("/e2e-listed")
        s.key("Escape")
        s.key("C-u")
        s.type("/help")
        s.key("Enter")
        scroll_help_to(s, "Prompt templates")
        s.wait_text("Listed template")


def t_template_added_mid_session_needs_no_restart(b, v):
    with Session(b) as s:
        s.wait_ready()
        install_template(s, "e2e-later", "TEMPLATE-LATER ran\n")
        s.type("/e2e-later")
        s.key("Enter")
        s.wait_text("TEMPLATE-LATER ran")


def t_built_in_command_wins_over_a_template(b, v):
    with Session(b) as s:
        install_template(s, "help", "SHADOW-TEMPLATE-RAN\n")
        s.wait_ready()
        s.type("/help")
        s.key("Enter")
        s.wait_text("Show this help overlay")
        assert "SHADOW-TEMPLATE-RAN" not in s.settle()


def t_bang_body_is_never_run_as_shell(b, v):
    with Session(b) as s:
        marker = os.path.join(s.work, "bang-ran")
        install_template(s, "e2e-bang", f"!touch {marker}\nTEMPLATE-BANG body\n")
        s.wait_ready()
        s.type("/e2e-bang")
        s.key("Enter")
        s.wait_text("TEMPLATE-BANG body")
        assert not os.path.exists(marker), "template body ran as an input-line shell command"


def t_project_command_wins_over_a_template(b, v):
    with Session(b) as s:
        install_commands(s, [
            {
                "name": "e2e-say",
                "description": "A project command",
                "command": "printf PROJECT-COMMAND-WINS",
            }
        ])
        install_template(s, "e2e-say", "TEMPLATE-SHADOW-RAN\n")
        s.wait_ready()
        s.type("/e2e-say")
        s.key("Enter")
        s.wait_text("PROJECT-COMMAND-WINS")
        assert "TEMPLATE-SHADOW-RAN" not in s.settle()
