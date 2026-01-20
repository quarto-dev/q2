import { useEffect } from 'react';
import './Toast.css';

interface ToastProps {
  message: string;
  visible: boolean;
  onHide: () => void;
  duration?: number;
}

export default function Toast({ message, visible, onHide, duration = 2000 }: ToastProps) {
  useEffect(() => {
    if (visible) {
      const timer = setTimeout(onHide, duration);
      return () => clearTimeout(timer);
    }
  }, [visible, onHide, duration]);

  if (!visible) return null;

  return (
    <div className="toast" role="status">
      {message}
    </div>
  );
}
