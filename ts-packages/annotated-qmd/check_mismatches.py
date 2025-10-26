import json, sys

def find_attrs_mismatches(obj, path=''):
    issues = []
    if isinstance(obj, dict):
        if 'attrS' in obj and 'c' in obj:
            c = obj.get('c', [])
            if isinstance(c, list) and len(c) > 0:
                attr = c[0] if isinstance(c[0], list) and len(c[0]) >= 3 else None
                if attr:
                    attr_s = obj['attrS']
                    classes = attr[1]
                    kvs = attr[2]

                    if len(classes) != len(attr_s.get('classes', [])):
                        issues.append({
                            'path': path,
                            'type': obj.get('t'),
                            'classes_in_attr': len(classes),
                            'classes_in_attrS': len(attr_s.get('classes', [])),
                            'classes': classes,
                            'attrS_classes': attr_s.get('classes', [])
                        })

                    if len(kvs) != len(attr_s.get('kvs', [])):
                        issues.append({
                            'path': path,
                            'type': obj.get('t'),
                            'kvs_in_attr': len(kvs),
                            'kvs_in_attrS': len(attr_s.get('kvs', [])),
                        })

        for k, v in obj.items():
            issues.extend(find_attrs_mismatches(v, f'{path}.{k}'))
    elif isinstance(obj, list):
        for i, item in enumerate(obj):
            issues.extend(find_attrs_mismatches(item, f'{path}[{i}]'))
    return issues

with open('examples/inline-types.json') as f:
    data = json.load(f)

issues = find_attrs_mismatches(data)
if issues:
    print(f'Found {len(issues)} mismatches:\n')
    for issue in issues:
        print(f'Type: {issue.get("type")}')
        print(f'Path: {issue["path"][:70]}')
        for k, v in issue.items():
            if k not in ['path', 'type']:
                print(f'  {k}: {v}')
        print()
else:
    print('No mismatches found')
