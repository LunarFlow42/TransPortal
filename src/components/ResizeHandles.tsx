import React from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { suppressAutoHide } from '../utils/autoHide';

type ResizeDirection =
  | 'East'
  | 'North'
  | 'NorthEast'
  | 'NorthWest'
  | 'South'
  | 'SouthEast'
  | 'SouthWest'
  | 'West';

interface ResizeHandlesProps {
  onResizeStart?: () => void;
}

/**
 * 8-direction native window resize handles for transparent borderless windows.
 * Placed around the perimeter to trigger native OS resize dragging.
 */
export const ResizeHandles: React.FC<ResizeHandlesProps> = ({ onResizeStart }) => {
  const handleMouseDown = (direction: ResizeDirection, event: React.MouseEvent) => {
    if (event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();
    suppressAutoHide(120_000);
    onResizeStart?.();
    void getCurrentWindow()
      .startResizeDragging(direction)
      .catch((error) => console.error('startResizeDragging failed:', error));
  };

  return (
    <div className="pointer-events-auto">
      {/* 4 Edges */}
      <div
        className="absolute inset-x-3 top-0 h-1.5 cursor-ns-resize z-50 select-none"
        onMouseDown={(e) => handleMouseDown('North', e)}
      />
      <div
        className="absolute inset-x-3 bottom-0 h-1.5 cursor-ns-resize z-50 select-none"
        onMouseDown={(e) => handleMouseDown('South', e)}
      />
      <div
        className="absolute inset-y-3 left-0 w-1.5 cursor-ew-resize z-50 select-none"
        onMouseDown={(e) => handleMouseDown('West', e)}
      />
      <div
        className="absolute inset-y-3 right-0 w-1.5 cursor-ew-resize z-50 select-none"
        onMouseDown={(e) => handleMouseDown('East', e)}
      />

      {/* 4 Corners */}
      <div
        className="absolute top-0 left-0 h-3 w-3 cursor-nwse-resize z-50 select-none"
        onMouseDown={(e) => handleMouseDown('NorthWest', e)}
      />
      <div
        className="absolute top-0 right-0 h-3 w-3 cursor-nesw-resize z-50 select-none"
        onMouseDown={(e) => handleMouseDown('NorthEast', e)}
      />
      <div
        className="absolute bottom-0 left-0 h-3 w-3 cursor-nesw-resize z-50 select-none"
        onMouseDown={(e) => handleMouseDown('SouthWest', e)}
      />
      <div
        className="absolute bottom-0 right-0 h-3 w-3 cursor-nwse-resize z-50 select-none"
        onMouseDown={(e) => handleMouseDown('SouthEast', e)}
      />
    </div>
  );
};
