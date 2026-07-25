// The attribute-label sampling facts. The labels themselves draw in the
// GPU pass (the renderer's label channel; text is assembled Rust-side in
// solarxy-web::attr_labels); what remains here is the tiny store the strip
// UI reads for the "N of M pts" sampling notice and the gear popover's
// live capacity, fed by attrPinStats host events.

import { create } from "zustand";

interface AttrPinStats {
  /** Labels the host is drawing (0 while the pin modes are off). */
  capacity: number;
  /** Displayed points in the scene; capacity < total means sampling. */
  total: number;
  set: (capacity: number, total: number) => void;
}

/** Published only on change so React re-renders on cooks, not frames. */
export const useAttrPinStats = create<AttrPinStats>((set, get) => ({
  capacity: 0,
  total: 0,
  set: (capacity, total) => {
    const s = get();
    if (s.capacity !== capacity || s.total !== total) set({ capacity, total });
  },
}));
