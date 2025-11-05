# Beads to GraphViz Converter

This script converts your beads issue tracker data into a GraphViz visualization.

## Usage

```bash
# Generate DOT file
python3 scripts/beads-to-graphviz.py > beads-graph.dot

# Generate SVG (recommended for web viewing)
dot -Tsvg beads-graph.dot -o beads-graph.svg

# Generate PNG
dot -Tpng beads-graph.dot -o beads-graph.png

# Generate PDF
dot -Tpdf beads-graph.dot -o beads-graph.pdf
```

Or all at once:

```bash
python3 scripts/beads-to-graphviz.py | tee beads-graph.dot | dot -Tsvg -o beads-graph.svg
```

## Visualization Key

### Node Colors

- **Fill Color** indicates status:
  - White: Open (not started)
  - Light yellow: In Progress (being worked on)
  - Gray (dashed border): Closed (completed)

- **Border Color** indicates issue type:
  - Red (#ff6b6b): Bug
  - Teal (#4ecdc4): Feature
  - Light green (#95e1d3): Task
  - Pink (#f38181): Epic
  - Pale green (#a8e6cf): Chore

### Node Shapes

- **Box**: Critical (P0) or High (P1) priority
- **Ellipse**: Medium (P2) or Low (P3) priority
- **Diamond**: Backlog (P4)

### Edges

- Arrows point from dependency to dependent issue
- For example: `A -> B` means issue A must be done before B can start

## Requirements

- Python 3
- GraphViz (`dot` command)
  - macOS: `brew install graphviz`
  - Ubuntu/Debian: `apt install graphviz`
  - Fedora: `dnf install graphviz`

## Notes

- The graph includes all issues from `bd list --json`
- Dependencies are fetched using `bd show <id> --json` for each issue
- Large graphs may need scaling (GraphViz will warn and auto-scale)
- For very large graphs, consider filtering by status or priority before visualization
