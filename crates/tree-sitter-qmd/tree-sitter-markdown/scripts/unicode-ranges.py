import unicodedata

def generate_ranges_excluding(category, exclude_codes):
    """Generate Unicode ranges for a category, excluding specific codes."""

    # Collect all characters in the category
    chars = []
    for i in range(0x110000):
        try:
            c = chr(i)
            if unicodedata.category(c) == category:
                chars.append(i)
        except:
            pass

    print(f"Total {category} characters: {len(chars)}")

    # Remove excluded characters
    removed = []
    for code in exclude_codes:
        if code in chars:
            chars.remove(code)
            removed.append(code)
            print(f"Removed U+{code:04X} {chr(code)} {unicodedata.name(chr(code))}")

    not_found = set(exclude_codes) - set(removed)
    if not_found:
        print(f"NOT in {category}:")
        for code in sorted(not_found):
            cat = unicodedata.category(chr(code))
            print(f"  U+{code:04X} {chr(code)} - category: {cat}")

    print(f"Total after exclusion: {len(chars)}")

    # Convert to ranges
    ranges = []
    if not chars:
        return ""

    start = chars[0]
    end = chars[0]

    for i in range(1, len(chars)):
        if chars[i] == end + 1:
            end = chars[i]
        else:
            ranges.append((start, end))
            start = chars[i]
            end = chars[i]
    ranges.append((start, end))

    print(f"Total ranges: {len(ranges)}")

    # Format for JavaScript regex
    parts = []
    for start, end in ranges:
        if start == end:
            parts.append(f"\\u{{{start:04X}}}")
        else:
            parts.append(f"\\u{{{start:04X}}}-\\u{{{end:04X}}}")

    output = "".join(parts)

    # Print full output
    print(f"\nFormatted for regex character class:")
    print(output)

    # Print in chunks for readability
    chunk_size = 10
    print(f"\nIn chunks of {chunk_size} ranges:")
    for i in range(0, len(parts), chunk_size):
        chunk = parts[i:i+chunk_size]
        print("  + \"" + "".join(chunk) + "\"")

    return output

# Main execution
if __name__ == "__main__":
    print("=" * 60)
    print("Sm (Math Symbol) - excluding | and ~")
    print("=" * 60)
    sm_result = generate_ranges_excluding('Sm', [0x007C, 0x007E])

    print("\n" + "=" * 60)
    print("Sk (Modifier Symbol) - excluding ^ and `")
    print("=" * 60)
    sk_result = generate_ranges_excluding('Sk', [0x005E, 0x0060])

    print("\n" + "=" * 60)
    print("Sc (Currency Symbol) - excluding $, small $, fullwidth $")
    print("=" * 60)
    sc_result = generate_ranges_excluding('Sc', [0x0024, 0xFE69, 0xFF04])

    print("\n" + "=" * 60)
    print("So (Other Symbol) - no exclusions")
    print("=" * 60)
    so_result = generate_ranges_excluding('So', [])