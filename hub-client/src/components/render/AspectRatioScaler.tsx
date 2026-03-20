import React, { useState, useEffect, useRef } from 'react';

interface AspectRatioScalerProps {
  /** Virtual width of the content */
  width: number;
  /** Virtual height of the content */
  height: number;
  /** Content to render at the virtual dimensions */
  children: React.ReactNode;
  /** Optional background color for the container */
  backgroundColor?: string;
}

/**
 * Component that maintains a fixed aspect ratio and scales its children
 * to fit within the parent container while preserving the aspect ratio.
 *
 * The children are rendered as if they're in a container of size width x height,
 * but the component scales and centers them to fit the actual parent.
 */
export function AspectRatioScaler({
  width,
  height,
  children,
  backgroundColor = 'transparent'
}: AspectRatioScalerProps) {
  const [scale, setScale] = useState(1);
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const updateScale = () => {
      if (!containerRef.current) return;

      const containerWidth = containerRef.current.clientWidth;
      const containerHeight = containerRef.current.clientHeight;

      // Don't calculate scale if container has no dimensions yet
      if (containerWidth === 0 || containerHeight === 0) return;

      // Calculate scale to fit both dimensions while maintaining aspect ratio
      const scaleX = containerWidth / width;
      const scaleY = containerHeight / height;
      const newScale = Math.min(scaleX, scaleY) * 0.95; // 95% to add some margin

      setScale(newScale);
    };

    updateScale();
    window.addEventListener('resize', updateScale);

    const resizeObserver = new ResizeObserver(updateScale);
    if (containerRef.current) {
      resizeObserver.observe(containerRef.current);
    }

    return () => {
      window.removeEventListener('resize', updateScale);
      resizeObserver.disconnect();
    };
  }, [width, height]);

  return (
    <div
      ref={containerRef}
      style={{
        width: '100%',
        height: '100%',
        flex: '1 1 auto',
        minWidth: 0,
        minHeight: 0,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        backgroundColor,
        overflow: 'hidden',
        position: 'relative'
      }}
    >
      <div
        style={{
          width: `${width}px`,
          height: `${height}px`,
          transform: `scale(${scale})`,
          transformOrigin: 'center center',
          position: 'relative'
        }}
      >
        {children}
      </div>
    </div>
  );
}
