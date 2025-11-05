#!/bin/bash
# Generate a GraphViz DOT file from beads issues

set -eo pipefail

# Color scheme for issue types
declare -A TYPE_COLORS=(
    ["bug"]="#ff6b6b"
    ["feature"]="#4ecdc4"
    ["task"]="#95e1d3"
    ["epic"]="#f38181"
    ["chore"]="#a8e6cf"
)

# Color scheme for statuses
declare -A STATUS_COLORS=(
    ["open"]="#ffffff"
    ["in_progress"]="#ffffcc"
    ["closed"]="#cccccc"
)

# Shape for priorities
declare -A PRIORITY_SHAPES=(
    [0]="box"        # Critical
    [1]="box"        # High
    [2]="ellipse"    # Medium
    [3]="ellipse"    # Low
    [4]="diamond"    # Backlog
)

echo "digraph beads {"
echo "  rankdir=TB;"
echo "  node [style=filled];"
echo ""

# Get all issues
issues=$(bd list --json)

# Create nodes
echo "$issues" | jq -r '.[] | @json' | while read -r issue_json; do
    id=$(echo "$issue_json" | jq -r '.id')
    title=$(echo "$issue_json" | jq -r '.title' | sed 's/"/\\"/g')
    status=$(echo "$issue_json" | jq -r '.status')
    priority=$(echo "$issue_json" | jq -r '.priority')
    issue_type=$(echo "$issue_json" | jq -r '.issue_type')

    # Get colors
    type_color="${TYPE_COLORS[$issue_type]:-#dddddd}"
    status_color="${STATUS_COLORS[$status]:-#ffffff}"
    shape="${PRIORITY_SHAPES[$priority]:-ellipse}"

    # Combine colors for gradient effect (type on border, status on fill)
    fillcolor="$status_color"
    color="$type_color"

    # Make closed issues lighter
    if [ "$status" = "closed" ]; then
        style="filled,dashed"
    else
        style="filled"
    fi

    # Truncate long titles
    short_title=$(echo "$title" | cut -c1-50)
    if [ ${#title} -gt 50 ]; then
        short_title="$short_title..."
    fi

    # Create label with ID and title
    label="$id\\n$short_title"

    echo "  \"$id\" [label=\"$label\", fillcolor=\"$fillcolor\", color=\"$color\", shape=$shape, style=\"$style\", penwidth=2];"
done

echo ""

# Create edges from dependencies
echo "$issues" | jq -r '.[] | @json' | while read -r issue_json; do
    id=$(echo "$issue_json" | jq -r '.id')

    # Get detailed issue info to see dependencies
    issue_detail=$(bd show "$id" --json)

    # Extract dependencies
    deps=$(echo "$issue_detail" | jq -r '.dependencies[]? | .id')

    if [ -n "$deps" ]; then
        echo "$deps" | while read -r dep_id; do
            echo "  \"$dep_id\" -> \"$id\" [label=\"discovered-from\"];"
        done
    fi
done

# Add legend
echo ""
echo "  subgraph cluster_legend {"
echo "    label=\"Legend\";"
echo "    style=filled;"
echo "    color=lightgrey;"
echo "    node [shape=plaintext];"
echo "    legend [label=<"
echo "      <TABLE BORDER=\"0\" CELLBORDER=\"1\" CELLSPACING=\"0\" CELLPADDING=\"4\">"
echo "        <TR><TD COLSPAN=\"2\"><B>Status</B></TD></TR>"
echo "        <TR><TD BGCOLOR=\"${STATUS_COLORS[open]}\">Open</TD><TD>Not started</TD></TR>"
echo "        <TR><TD BGCOLOR=\"${STATUS_COLORS[in_progress]}\">In Progress</TD><TD>Being worked on</TD></TR>"
echo "        <TR><TD BGCOLOR=\"${STATUS_COLORS[closed]}\" STYLE=\"dashed\">Closed</TD><TD>Completed</TD></TR>"
echo "        <TR><TD COLSPAN=\"2\"><B>Type (border color)</B></TD></TR>"
echo "        <TR><TD><FONT COLOR=\"${TYPE_COLORS[bug]}\">■</FONT> Bug</TD><TD>Something broken</TD></TR>"
echo "        <TR><TD><FONT COLOR=\"${TYPE_COLORS[feature]}\">■</FONT> Feature</TD><TD>New functionality</TD></TR>"
echo "        <TR><TD><FONT COLOR=\"${TYPE_COLORS[task]}\">■</FONT> Task</TD><TD>Work item</TD></TR>"
echo "        <TR><TD><FONT COLOR=\"${TYPE_COLORS[epic]}\">■</FONT> Epic</TD><TD>Large feature</TD></TR>"
echo "        <TR><TD><FONT COLOR=\"${TYPE_COLORS[chore]}\">■</FONT> Chore</TD><TD>Maintenance</TD></TR>"
echo "        <TR><TD COLSPAN=\"2\"><B>Priority (shape)</B></TD></TR>"
echo "        <TR><TD>Box</TD><TD>P0-P1 (Critical/High)</TD></TR>"
echo "        <TR><TD>Ellipse</TD><TD>P2-P3 (Medium/Low)</TD></TR>"
echo "        <TR><TD>Diamond</TD><TD>P4 (Backlog)</TD></TR>"
echo "      </TABLE>"
echo "    >];"
echo "  }"

echo "}"
