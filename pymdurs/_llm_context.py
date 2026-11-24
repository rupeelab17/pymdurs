"""Render the installed API reference and examples as Markdown."""

from __future__ import annotations

from importlib.resources import files

_STUBS = (
    "__init__.pyi",
    "pymdurs.pyi",
    "geometric/__init__.pyi",
    "thermal/__init__.pyi",
)


def llm_context() -> str:
    """Return the packaged type stubs and UTCI workflow without using the network.

    Returns:
        Markdown ready to use as a local reference or share with an assistant.
    """
    root = files("pymdurs")
    sections = [
        "# pymdurs\n\n"
        "> Python bindings for urban geospatial data collection and processing. "
        "Use the typed signatures below and the UTCI example for workflow order. "
        "SOLWEIG, not pymdurs, computes Tmrt and UTCI in this example."
    ]
    for path in (*_STUBS, "examples/utci_bbox.py"):
        source = root.joinpath(*path.split("/")).read_text(encoding="utf-8")
        source = "\n".join(line.rstrip() for line in source.splitlines()).rstrip("\n")
        sections.append(f"## `pymdurs/{path}`\n\n```python\n{source}\n```")
    return "\n\n".join(sections) + "\n"
