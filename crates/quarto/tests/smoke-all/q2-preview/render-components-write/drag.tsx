const React = window.React;
const {
    renderChildren,
    blockStyle
} = window.__Q2_PREVIEW_RENDERER__;

export const Div = (args) => {
    const attrs = new Map(args.node.c[0][2])
    const initialX = Number(attrs.get('x') ?? 0)
    const initialY = Number(attrs.get('y') ?? 0)

    const [x, setX] = React.useState(initialX)
    const [y, setY] = React.useState(initialY)
    const dragStartRef = React.useRef(null)
    const divRef = React.useRef(null)

    // Sync x and y when attrs change externally (not during drag)
    React.useEffect(() => {
        if (!dragStartRef.current) {
            setX(initialX)
            setY(initialY)
        }
    }, [initialX, initialY])

    const getScale = (el) => {
        let scale = 1
        let current = el
        while (current && current !== document.body) {
            const style = window.getComputedStyle(current)
            const transform = style.transform
            if (transform && transform !== 'none') {
                const matrix = new DOMMatrix(transform)
                scale *= matrix.a
            }
            current = current.parentElement
        }
        return scale
    }

    React.useEffect(() => {
        const handleMouseMove = (e) => {
            if (dragStartRef.current) {
                const scale = getScale(divRef.current)
                const dx = (e.clientX - dragStartRef.current.mouseX) / scale
                const dy = (e.clientY - dragStartRef.current.mouseY) / scale
                setX(dragStartRef.current.startX + dx)
                setY(dragStartRef.current.startY + dy)
            }
        }

        const handleMouseUp = () => {
            if (dragStartRef.current) {
                args.node.c[0][2] = [['x', x + ''], ['y', y + '']]
                args.setLocalAst(args.node)
                dragStartRef.current = null
            }
        }

        window.addEventListener('mousemove', handleMouseMove)
        window.addEventListener('mouseup', handleMouseUp)
        return () => {
            window.removeEventListener('mousemove', handleMouseMove)
            window.removeEventListener('mouseup', handleMouseUp)
        }
    }, [x, y])

    const t = `translate(${x}px, ${y}px)`

    return <div ref={divRef} style={{ transform: t, ...blockStyle, position: 'relative', outline: '1px solid #999', zIndex: 1000 }}>
        <div
            style={{
                position: 'absolute',
                top: '-8px',
                left: '-8px',
                width: '16px',
                height: '16px',
                borderRadius: '50%',
                backgroundColor: '#666',
                cursor: 'grab',
                zIndex: 10000,
            }}
            onMouseDown={(e) => {
                dragStartRef.current = {
                    mouseX: e.clientX,
                    mouseY: e.clientY,
                    startX: x,
                    startY: y
                }
            }}
        />
        {renderChildren(args)}
    </div>
}