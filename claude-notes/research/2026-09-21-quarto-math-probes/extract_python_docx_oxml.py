"""Statically extract python-docx's declarative element model (no lxml needed)."""
import ast, json, sys, pathlib
root = pathlib.Path(sys.argv[1]) / "src/docx/oxml"
CHILD_KINDS = {"ZeroOrOne","ZeroOrMore","OneOrMore","OneAndOnlyOne","ZeroOrOneChoice","Choice"}
ATTR_KINDS = {"OptionalAttribute","RequiredAttribute"}
out = {}
def lit(n):
    try: return ast.literal_eval(n)
    except Exception: return ast.unparse(n)
for f in sorted(root.rglob("*.py")):
    tree = ast.parse(f.read_text())
    for cls in [n for n in ast.walk(tree) if isinstance(n, ast.ClassDef) and n.name.startswith("CT_")]:
        rec = {"file": str(f.relative_to(root)), "tag_seq": None, "children": [], "attrs": [], "templates": []}
        for node in cls.body:
            if isinstance(node, ast.Assign) and any(isinstance(t, ast.Name) and t.id=="_tag_seq" for t in node.targets):
                rec["tag_seq"] = lit(node.value)
            tgt = val = None
            if isinstance(node, ast.Assign) and len(node.targets)==1 and isinstance(node.targets[0], ast.Name):
                tgt, val = node.targets[0].id, node.value
            elif isinstance(node, ast.AnnAssign) and isinstance(node.target, ast.Name) and node.value is not None:
                tgt, val = node.target.id, node.value
            if isinstance(val, ast.Call) and isinstance(val.func, ast.Name):
                kind = val.func.id
                args = [lit(a) for a in val.args]
                kw = {k.arg: lit(k.value) for k in val.keywords}
                if kind in CHILD_KINDS:
                    rec["children"].append({"prop": tgt, "kind": kind, "args": args, **kw})
                elif kind in ATTR_KINDS:
                    rec["attrs"].append({"prop": tgt, "kind": kind, "args": args, **kw})
            if isinstance(node, ast.FunctionDef) and node.name.startswith(("new", "_", "add")):
                src = ast.unparse(node)
                if "parse_xml(" in src or "<w:" in src or "<wp:" in src or "<pic:" in src:
                    rec["templates"].append(node.name)
        out[cls.name] = rec
json.dump(out, open(sys.argv[2], "w"), indent=1)
n = len(out); seq = sum(1 for r in out.values() if r["tag_seq"]); ch = sum(len(r["children"]) for r in out.values()); at = sum(len(r["attrs"]) for r in out.values()); tp = sum(len(r["templates"]) for r in out.values())
print(f"classes={n} with_tag_seq={seq} child_decls={ch} attr_decls={at} xml_template_methods={tp}")
