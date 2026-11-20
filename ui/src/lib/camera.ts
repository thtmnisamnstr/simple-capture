import { ratio, type Ratio } from './geometry.ts';
import type { CameraShape } from './types.ts';

export const CAMERA_SHAPES: { value: CameraShape; label: string; title: string }[] = [
  { value: 'circle', label: '●', title: 'Circle' },
  { value: 'rounded', label: '▢', title: 'Rounded square' },
  { value: 'square', label: '■', title: 'Square' },
  { value: 'rect_16_9', label: '16:9', title: 'Wide' },
  { value: 'rect_4_3', label: '4:3', title: '4:3' },
  { value: 'rect_3_4', label: '3:4', title: '3:4' },
  { value: 'rect_9_16', label: '9:16', title: 'Tall' },
];

export function shapeAspect(shape: CameraShape): Ratio {
  switch (shape) {
    case 'rect_4_3':
      return ratio(4, 3);
    case 'rect_16_9':
      return ratio(16, 9);
    case 'rect_3_4':
      return ratio(3, 4);
    case 'rect_9_16':
      return ratio(9, 16);
    default:
      return ratio(1, 1);
  }
}
