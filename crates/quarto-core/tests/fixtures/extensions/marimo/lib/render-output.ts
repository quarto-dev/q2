/*
 * render-output.ts
 *
 * Renders a single marimo output to markdown.
 */

// Type for marimo cell output from extract.py
export interface MarimoOutput {
  type: "html" | "figure" | "para" | "blockquote";
  value: string;
  display_code: boolean;
  reactive: boolean;
  code: string;
}

export async function renderOutput(
  output: MarimoOutput,
  mimeSensitive: boolean,
  htmlToMarkdown: (html: string) => Promise<string> = async (h) => h
): Promise<string> {
  let result = "";

  // Add code block if display_code is true
  if (output.display_code && output.code) {
    result += "```python\n" + output.code + "\n```\n\n";
  }

  // Render based on output type and format
  if (!mimeSensitive) {
    // HTML output - wrap everything in raw HTML blocks
    if (output.value) {
      result += "```{=html}\n" + output.value + "\n```\n\n";
    }
  } else {
    // PDF/LaTeX output - handle based on type
    switch (output.type) {
      case "figure":
        if (output.value) {
          result += `![Generated Figure](${output.value})\n\n`;
        }
        break;

      case "para":
        if (output.value) {
          result += output.value + "\n\n";
        }
        break;

      case "blockquote":
        if (output.value) {
          result += "> " + output.value + "\n\n";
        }
        break;

      case "html":
      default:
        if (output.value) {
          // Check if HTML contains a table - if so, use raw HTML block
          if (/<table[\s>]/i.test(output.value)) {
            result += "```{=html}\n" + output.value + "\n```\n\n";
          } else {
            // Convert HTML to markdown using pandoc
            const markdown = await htmlToMarkdown(output.value);
            result += markdown + "\n\n";
          }
        }
        break;
    }
  }

  return result;
}
