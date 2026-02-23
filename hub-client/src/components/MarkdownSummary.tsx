import { useRef, useCallback } from 'react';
import './MarkdownSummary.css';

interface MarkdownSummaryProps {
  /** The markdown content to display */
  content: string;
  /** Callback when user clicks to navigate */
  onLineClick?: (lineNumber: number) => void;
}

/**
 * A simplified, zoomed-out view of markdown content.
 * Displays the text at a small scale for use as a navigation minimap.
 */
export default function MarkdownSummary({ content, onLineClick }: MarkdownSummaryProps) {
  const containerRef = useRef<HTMLDivElement>(null);

  const handleClick = useCallback((e: React.MouseEvent) => {
    if (!containerRef.current || !onLineClick) return;

    const container = containerRef.current;
    const rect = container.getBoundingClientRect();
    const clickY = e.clientY - rect.top + container.scrollTop;

    // Estimate line number from click position (assuming ~12px line height at scale)
    const lineHeight = 12;
    const lineNumber = Math.floor(clickY / lineHeight) + 1;
    onLineClick(lineNumber);
  }, [onLineClick]);

  return (
    <div className="markdown-summary" ref={containerRef} onClick={handleClick}>
      <pre className="markdown-summary-content">{content}</pre>
    </div>
  );
}
