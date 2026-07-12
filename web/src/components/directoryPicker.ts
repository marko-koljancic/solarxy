// React's TS attributes lack the non-standard (but universally supported
// in the WebGPU browser floor) folder-picker flag on file inputs; spread
// this in untyped wherever a hidden input should select a whole folder.

export const DIRECTORY_PICKER = {
  webkitdirectory: "",
} as unknown as React.InputHTMLAttributes<HTMLInputElement>;
