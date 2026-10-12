import { beforeEach, describe, expect, it, vi } from 'vitest';
import { useImageZoomPan } from './useImageZoomPan';

// Mock React hooks to run in standard Node test environment without full DOM
const stateStore = {
  states: [] as unknown[],
  index: 0,
};

vi.mock('react', async (importOriginal) => {
  const actual = await importOriginal<typeof import('react')>();
  return {
    ...actual,
    useState: (initial: unknown) => {
      const currentIndex = stateStore.index++;
      if (stateStore.states.length <= currentIndex) {
        stateStore.states[currentIndex] = initial;
      }
      const setState = (nextVal: unknown) => {
        stateStore.states[currentIndex] =
          typeof nextVal === 'function'
            ? (nextVal as (prev: unknown) => unknown)(stateStore.states[currentIndex])
            : nextVal;
      };
      return [stateStore.states[currentIndex], setState];
    },
    useRef: (initial: unknown) => ({ current: initial }),
    useCallback: (fn: (...args: unknown[]) => unknown) => fn,
    useEffect: () => {},
  };
});

describe('useImageZoomPan', () => {
  beforeEach(() => {
    stateStore.states = [];
    stateStore.index = 0;
  });

  it('provides default initial values', () => {
    stateStore.index = 0;
    const hook = useImageZoomPan();

    expect(hook.scale).toBe(1.0);
    expect(hook.offset).toEqual({ x: 0, y: 0 });
    expect(hook.isDragging).toBe(false);
    expect(hook.isZoomed).toBe(false);
    expect(hook.zoomPercent).toBe('100%');
    expect(hook.transformStyle.transform).toBe('translate3d(0px, 0px, 0) scale(1)');
  });

  it('clamps setScale within minScale and maxScale', () => {
    stateStore.index = 0;
    const hook = useImageZoomPan({ minScale: 0.8, maxScale: 3.0 });

    // Below minScale
    hook.setScale(0.2);
    expect(stateStore.states[0]).toBe(0.8);

    // Above maxScale
    hook.setScale(5.0);
    expect(stateStore.states[0]).toBe(3.0);

    // Valid scale
    hook.setScale(1.5);
    expect(stateStore.states[0]).toBe(1.5);
  });

  it('handles wheel zoom math correctly', () => {
    stateStore.index = 0;
    const hook = useImageZoomPan();

    // Mock container bounding rect
    hook.containerRef.current = {
      getBoundingClientRect: () => ({
        left: 0,
        top: 0,
        width: 400,
        height: 300,
        right: 400,
        bottom: 300,
        x: 0,
        y: 0,
        toJSON: () => {},
      }),
      clientWidth: 400,
      clientHeight: 300,
    } as unknown as HTMLDivElement;

    // Zoom in with wheel (deltaY < 0)
    hook.onWheel({
      preventDefault: vi.fn(),
      stopPropagation: vi.fn(),
      deltaY: -100,
      clientX: 200,
      clientY: 150,
    } as unknown as React.WheelEvent<HTMLElement>);

    // Scale should have increased from 1.0 to 1.15
    expect(stateStore.states[0]).toBeCloseTo(1.15);

    // Reset zoom
    hook.resetZoom();
    expect(stateStore.states[0]).toBe(1.0);
    expect(stateStore.states[1]).toEqual({ x: 0, y: 0 });
  });

  it('handles double click reset', () => {
    stateStore.index = 0;
    stateStore.states = [2.0, { x: 50, y: 50 }, false];
    const hook = useImageZoomPan();

    hook.onDoubleClick({
      preventDefault: vi.fn(),
      stopPropagation: vi.fn(),
    } as unknown as React.MouseEvent<HTMLElement>);

    expect(stateStore.states[0]).toBe(1.0);
    expect(stateStore.states[1]).toEqual({ x: 0, y: 0 });
  });
});
