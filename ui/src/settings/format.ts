/** How the settings page shows the value of a slider. */

export const px = (v: number): string => `${Math.round(v)} px`;
export const percent = (v: number): string => `${Math.round(v * 100)} %`;
export const scale = (v: number): string => `${v.toFixed(2)}×`;
export const icons = (v: number): string => (v === 1 ? '1 icon' : `${v} icons`);
export const millis = (v: number): string => `${Math.round(v)} ms`;
