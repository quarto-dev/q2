#!/usr/bin/env python3
"""Generate a GraphViz DOT file from beads issues."""

import json
import subprocess
import sys
from typing import Dict, List, Any

# Color scheme for issue types
TYPE_COLORS = {
    "bug": "#ff6b6b",
    "feature": "#4ecdc4",
    "task": "#95e1d3",
    "epic": "#f38181",
    "chore": "#a8e6cf",
}

# Color scheme for statuses
STATUS_COLORS = {
    "open": "#ffffff",
    "in_progress": "#ffffcc",
    "closed": "#cccccc",
}

# Shape for priorities
PRIORITY_SHAPES = {
    0: "box",       # Critical
    1: "box",       # High
    2: "ellipse",   # Medium
    3: "ellipse",   # Low
    4: "diamond",   # Backlog
}


def run_bd_command(args: List[str]) -> Any:
    """Run a bd command and return JSON output."""
    result = subprocess.run(
        ["bd"] + args + ["--json"],
        capture_output=True,
        text=True,
        check=True
    )
    return json.loads(result.stdout)


def escape_label(text: str) -> str:
    """Escape text for GraphViz labels."""
    return text.replace('"', '\\"').replace('\n', '\\n')


def truncate_title(title: str, max_len: int = 50) -> str:
    """Truncate title if too long."""
    if len(title) <= max_len:
        return title
    return title[:max_len] + "..."


def generate_dot() -> str:
    """Generate GraphViz DOT format from beads issues."""
    lines = []
    lines.append("digraph beads {")
    lines.append("  rankdir=TB;")
    lines.append("  node [style=filled];")
    lines.append("")

    # Get all issues
    issues = run_bd_command(["list"])

    # Create nodes
    for issue in issues:
        issue_id = issue["id"]
        title = issue["title"]
        status = issue["status"]
        priority = issue["priority"]
        issue_type = issue["issue_type"]

        # Get colors and shape
        type_color = TYPE_COLORS.get(issue_type, "#dddddd")
        status_color = STATUS_COLORS.get(status, "#ffffff")
        shape = PRIORITY_SHAPES.get(priority, "ellipse")

        # Style based on status
        if status == "closed":
            style = "filled,dashed"
        else:
            style = "filled"

        # Create label
        short_title = truncate_title(title)
        label = f"{issue_id}\\n{escape_label(short_title)}"

        # Generate node
        lines.append(
            f'  "{issue_id}" ['
            f'label="{label}", '
            f'fillcolor="{status_color}", '
            f'color="{type_color}", '
            f'shape={shape}, '
            f'style="{style}", '
            f'penwidth=2];'
        )

    lines.append("")

    # Create edges from dependencies
    for issue in issues:
        issue_id = issue["id"]

        # Get detailed issue info to see dependencies
        try:
            issue_detail = run_bd_command(["show", issue_id])
            deps = issue_detail.get("dependencies", [])

            for dep in deps:
                dep_id = dep["id"]
                # Arrow goes from dependency to dependent
                lines.append(f'  "{dep_id}" -> "{issue_id}";')
        except Exception as e:
            print(f"Warning: Could not get dependencies for {issue_id}: {e}", file=sys.stderr)

    # Add legend
    lines.append("")
    lines.append("  subgraph cluster_legend {")
    lines.append("    label=\"Legend\";")
    lines.append("    style=filled;")
    lines.append("    color=lightgrey;")
    lines.append("    node [shape=plaintext];")
    lines.append("    legend [label=<")
    lines.append("      <TABLE BORDER=\"0\" CELLBORDER=\"1\" CELLSPACING=\"0\" CELLPADDING=\"4\">")
    lines.append("        <TR><TD COLSPAN=\"2\"><B>Status</B></TD></TR>")
    lines.append(f"        <TR><TD BGCOLOR=\"{STATUS_COLORS['open']}\">Open</TD><TD>Not started</TD></TR>")
    lines.append(f"        <TR><TD BGCOLOR=\"{STATUS_COLORS['in_progress']}\">In Progress</TD><TD>Being worked on</TD></TR>")
    lines.append(f"        <TR><TD BGCOLOR=\"{STATUS_COLORS['closed']}\" STYLE=\"dashed\">Closed</TD><TD>Completed</TD></TR>")
    lines.append("        <TR><TD COLSPAN=\"2\"><B>Type (border color)</B></TD></TR>")

    for issue_type, color in TYPE_COLORS.items():
        lines.append(f"        <TR><TD><FONT COLOR=\"{color}\">■</FONT> {issue_type.title()}</TD><TD></TD></TR>")

    lines.append("        <TR><TD COLSPAN=\"2\"><B>Priority (shape)</B></TD></TR>")
    lines.append("        <TR><TD>Box</TD><TD>P0-P1 (Critical/High)</TD></TR>")
    lines.append("        <TR><TD>Ellipse</TD><TD>P2-P3 (Medium/Low)</TD></TR>")
    lines.append("        <TR><TD>Diamond</TD><TD>P4 (Backlog)</TD></TR>")
    lines.append("      </TABLE>")
    lines.append("    >];")
    lines.append("  }")

    lines.append("}")

    return "\n".join(lines)


if __name__ == "__main__":
    try:
        dot = generate_dot()
        print(dot)
    except subprocess.CalledProcessError as e:
        print(f"Error running bd command: {e}", file=sys.stderr)
        sys.exit(1)
    except Exception as e:
        print(f"Error: {e}", file=sys.stderr)
        sys.exit(1)
