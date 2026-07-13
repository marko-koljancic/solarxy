// The live gizmo drag readout: a small chip showing the delta while a handle is
// being dragged ("X +1.250 m", "Z +42.0 deg", "1.250x").
//
// The thing that separates a gizmo you can eyeball from one you can actually
// work with is seeing the number without opening a panel. The parameter panel
// deliberately does NOT update mid-drag (a drag is one undo step, committed on
// release), so without this the user is flying blind until they let go.
//
// The value is POLLED from the host once per frame in `runFrame`, not pushed:
// `pointerMove` stays void so a drag keeps costing zero boundary crossings.

import { useViewState } from "../store/viewState";

export function GizmoReadout() {
  const text = useViewState((s) => s.gizmoReadout);
  if (!text) return null;
  return (
    <div className="gizmo-readout" role="status" aria-live="off">
      {text}
    </div>
  );
}
